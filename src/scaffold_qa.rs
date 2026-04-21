//! Create a QA repo from a named template directory under qa/templates/.
//!
//! Usage: bin/flow scaffold-qa --template <name> --repo <owner/repo>
//!
//! Reads template files from qa/templates/<name>/, creates a GitHub repo,
//! writes the files, tags seed, and creates issues from .qa/issues.json.
//! The `<name>` selects a directory under qa/templates/. Each template
//! ships its own bin/* scripts so the QA repo exercises a real
//! toolchain end-to-end.
//!
//! Test hooks via environment variables (production never sets these):
//! - `FLOW_SCAFFOLD_TEMPLATES_BASE`: overrides `default_templates_base()`
//!   so integration tests can point at a fixture-built templates tree.
//! - `FLOW_SCAFFOLD_CLONE_DIR`: overrides `default_clone_dir()` so tests
//!   can pre-seed or inspect the clone directory.

use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::qa_reset::default_runner;

#[derive(Parser, Debug)]
#[command(name = "scaffold-qa", about = "Create a QA repo from templates")]
pub struct Args {
    /// Template directory name under qa/templates/
    #[arg(long)]
    pub template: String,

    /// GitHub repo (owner/name)
    #[arg(long)]
    pub repo: String,
}

/// Walk `template_dir` recursively and collect every file's content
/// keyed by path relative to `template_dir`. Returns a `BTreeMap` so
/// ordering is deterministic.
///
/// The caller is responsible for checking that `template_dir` exists
/// and is a directory. An empty-tree input returns an empty map.
fn find_templates(template_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut templates = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![template_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry.expect("ReadDir entry iteration failed on local filesystem");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(template_dir)
                    .expect("read_dir entry must live under template_dir")
                    .to_string_lossy()
                    .into_owned();
                let content = std::fs::read_to_string(&path)?;
                templates.insert(rel, content);
            }
        }
    }
    Ok(templates)
}

/// Scaffold a QA repo from templates. Private — production uses
/// [`run_impl`] as the single entry point.
///
/// 1. gh repo create
/// 2. Write template files to clone_dir
/// 3. git init, add, commit, tag seed, push
/// 4. Create issues from .qa/issues.json
fn scaffold_qa(template: &str, repo: &str, templates_base: &Path, clone_dir: &Path) -> Value {
    let template_dir = templates_base.join(template);
    if !template_dir.is_dir() {
        return json!({
            "status": "error",
            "message": format!("Unknown template: {}", template)
        });
    }

    let templates = match find_templates(&template_dir) {
        Ok(t) => t,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to read templates: {}", e)
            })
        }
    };

    let result = default_runner(
        &["gh", "repo", "create", repo, "--public", "--confirm"],
        None,
    );
    if !result.success {
        return json!({
            "status": "error",
            "message": format!("gh repo create failed: {}", result.stderr.trim())
        });
    }

    // clone_dir creation: fresh UUID subdir of temp_dir() (the production
    // default) or a test-provided override. Both paths are always
    // writable when scaffold-qa is invoked correctly; an Err here is a
    // programmer-visible panic per
    // `.claude/rules/testability-means-simplicity.md`.
    std::fs::create_dir_all(clone_dir).expect("scaffold-qa clone_dir must be creatable");

    let mut issues_data: Vec<Value> = Vec::new();
    for (rel_path, content) in &templates {
        // rel_path is a BTreeMap key produced by find_templates from a
        // strip_prefix'd DirEntry path, so it is always a non-empty
        // relative path. `rfind('/')` yields the parent portion when
        // present; a rel_path without '/' is top-level and needs no
        // extra mkdir (clone_dir already exists above).
        if let Some(slash_pos) = rel_path.rfind('/') {
            let parent = clone_dir.join(&rel_path[..slash_pos]);
            let _ = std::fs::create_dir_all(&parent);
        }
        let file_path = clone_dir.join(rel_path);
        // fs::write into a fresh clone_dir with valid parent dirs cannot
        // fail in practice; a failure here is a programmer-visible panic.
        std::fs::write(&file_path, content).expect("scaffold-qa template write must succeed");

        if rel_path.starts_with("bin/") {
            let _ = std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o755));
        }

        if rel_path == ".qa/issues.json" {
            issues_data = serde_json::from_str(content).unwrap_or_default();
        }
    }

    let remote_url = format!("https://github.com/{}.git", repo);
    let git_commands: Vec<Vec<&str>> = vec![
        vec!["git", "init", "-b", "main"],
        vec!["git", "add", "-A"],
        vec!["git", "commit", "-m", "Initial commit"],
        vec!["git", "tag", "seed"],
        vec!["git", "remote", "add", "origin", &remote_url],
        vec!["git", "push", "-u", "origin", "main", "--tags"],
    ];
    for cmd in &git_commands {
        let result = default_runner(cmd, Some(clone_dir));
        if !result.success {
            return json!({
                "status": "error",
                "message": format!("{} failed: {}", cmd[..3].join(" "), result.stderr.trim())
            });
        }
    }

    let mut issues_created = 0;
    for issue in &issues_data {
        let title = issue["title"].as_str().unwrap_or("");
        let body = issue["body"].as_str().unwrap_or("");
        let mut labels: Vec<&str> = Vec::new();
        if let Some(arr) = issue["labels"].as_array() {
            for v in arr {
                if let Some(s) = v.as_str() {
                    labels.push(s);
                }
            }
        }

        let mut cmd: Vec<&str> = vec![
            "gh", "issue", "create", "--repo", repo, "--title", title, "--body", body,
        ];
        for label in &labels {
            cmd.push("--label");
            cmd.push(label);
        }

        let r = default_runner(&cmd, None);
        if r.success {
            issues_created += 1;
        }
    }

    json!({
        "status": "ok",
        "repo": repo,
        "issues_created": issues_created
    })
}

/// Resolve the qa/templates/ base directory. Honors the
/// `FLOW_SCAFFOLD_TEMPLATES_BASE` env var for test overrides; otherwise
/// walks from `current_exe` up to the repo root (three levels up from
/// target/{release,debug}/flow-rs). Unreachable failures
/// (missing executable path, binary at filesystem root) panic with a
/// clear message — these cannot happen when flow-rs is invoked normally
/// via `bin/flow`.
fn default_templates_base() -> PathBuf {
    if let Ok(override_path) = std::env::var("FLOW_SCAFFOLD_TEMPLATES_BASE") {
        return PathBuf::from(override_path);
    }
    let exe = std::env::current_exe().expect("scaffold-qa: cannot resolve current executable");
    let root = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("scaffold-qa: binary path has no repo-root ancestor");
    root.join("qa").join("templates")
}

/// Choose a fresh clone directory. Honors the
/// `FLOW_SCAFFOLD_CLONE_DIR` env var for test overrides; otherwise
/// returns a fresh UUID-suffixed path under `std::env::temp_dir()`.
fn default_clone_dir() -> PathBuf {
    if let Ok(override_path) = std::env::var("FLOW_SCAFFOLD_CLONE_DIR") {
        return PathBuf::from(override_path);
    }
    std::env::temp_dir().join(format!("flow-qa-{}", uuid::Uuid::new_v4()))
}

/// CLI entry point.
///
/// Returns `Ok(Value)` for both success and status-error responses.
/// Returns `Err(String)` only for infrastructure failures (none today).
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let templates_base = default_templates_base();
    let clone_dir = default_clone_dir();
    Ok(scaffold_qa(
        &args.template,
        &args.repo,
        &templates_base,
        &clone_dir,
    ))
}
