//! Consolidated setup for FLOW Prime.
//!
//! Merges permissions into `.claude/settings.json`, writes `.flow.json`
//! version marker, updates `.git/info/exclude`, installs hooks and
//! launcher, and copies the bin/* stubs from `assets/bin-stubs/` into
//! `<project_root>/bin/`. Does NOT commit — the skill handles `git add`
//! + `commit`.
//!
//! Usage: `bin/flow prime-setup <project_root>`
//!
//! Output (JSON to stdout):
//!   Success: `{"status": "ok", "settings_merged": true, ...}`
//!   Failure: `{"status": "error", "message": "..."}`

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use clap::Args as ClapArgs;
use regex::Regex;
use serde_json::{json, Value};

use crate::prime_check::{
    compute_config_hash, compute_setup_hash, EXCLUDE_ENTRIES, FLOW_DENY, UNIVERSAL_ALLOW,
};
use crate::utils::{permission_to_regex, plugin_root, read_version};

/// Structural regex matching `<Type>(<inner>)`. Cached because `is_subsumed`
/// invokes it once per candidate plus once per same-type entry in the
/// existing set; with the FLOW universal allow list at ~80 Bash entries
/// the merge inside `merge_settings` was performing thousands of fresh
/// compiles per process invocation.
fn outer_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\w+)\((.+)\)$").expect("outer regex must compile"))
}

/// Process-level cache of compiled subsumption regexes for every entry in
/// `UNIVERSAL_ALLOW`. The merge loop in `merge_settings` does an O(N²) walk
/// where each pair of same-type entries needs a matching regex; without
/// this cache the dynamic compile in `permission_to_regex` ran ~6400 times
/// per `prime-setup` invocation, pushing the instrumented test binary past
/// nextest's slow-timeout. `is_subsumed` falls back to a per-call compile
/// for entries that originate from the user's existing `settings.json`
/// rather than from the FLOW universal list.
fn allow_regex_map() -> &'static HashMap<&'static str, Regex> {
    static MAP: OnceLock<HashMap<&'static str, Regex>> = OnceLock::new();
    MAP.get_or_init(|| {
        UNIVERSAL_ALLOW
            .iter()
            .filter_map(|s| permission_to_regex(s).map(|r| (*s, r)))
            .collect()
    })
}

/// Pre-commit hook script content — installed at `.git/hooks/pre-commit`.
/// Blocks direct `git commit` when a FLOW feature is active on the
/// current branch (detected by `.flow-states/<branch>.json` existence)
/// unless `.flow-commit-msg` is present (set by `/flow:flow-commit`).
pub const PRE_COMMIT_HOOK: &str = r#"#!/usr/bin/env bash
# .git/hooks/pre-commit — installed by /flow:flow-prime
# Only enforce when the current branch has an active FLOW feature
branch=$(git symbolic-ref --short HEAD 2>/dev/null)
if [ -n "$branch" ] && [ -f ".flow-states/${branch}.json" ] && [ ! -f .flow-commit-msg ]; then
  echo "BLOCKED: FLOW feature in progress on ${branch}. Commits must go through /flow:flow-commit."
  echo "The file .flow-commit-msg was not found — this looks like a direct git commit."
  exit 1
fi
"#;

/// Global FLOW launcher script content — installed at `~/.local/bin/flow`.
/// Reads `plugin_root` from the project's `.flow.json` to locate
/// the actual `bin/flow` dispatcher.
pub const LAUNCHER_SCRIPT: &str = r#"#!/usr/bin/env bash
# Global FLOW launcher — installed by /flow:flow-prime
# Reads plugin_root from .flow.json in the current git repo
set -euo pipefail

project_root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "Error: not inside a git repository" >&2
  exit 1
}

flow_json="$project_root/.flow.json"
if [ ! -f "$flow_json" ]; then
  echo "Error: $flow_json not found — run /flow:flow-prime in this project first" >&2
  exit 1
fi

plugin_root=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('plugin_root',''))" \
  "$flow_json" 2>/dev/null) || plugin_root=""
if [ -z "$plugin_root" ]; then
  echo "Error: plugin_root not found in $flow_json — run /flow:flow-prime to update" >&2
  exit 1
fi

if [ ! -d "$plugin_root" ]; then
  echo "Error: plugin path $plugin_root does not exist — run /flow:flow-prime to update" >&2
  exit 1
fi

exec "$plugin_root/bin/flow" "$@"
"#;

/// Check if any entry in `existing_set` pattern-subsumes `candidate`.
///
/// Uses `permission_to_regex()` to test whether an existing broader pattern
/// (e.g. `Agent(*)`) matches the candidate's concrete form (e.g.
/// `Agent(flow:ci-fixer)`). Only checks same-type entries (e.g. Agent vs
/// Agent, Read vs Read); never matches across types (Agent vs Bash).
pub fn is_subsumed(candidate: &str, existing_set: &HashSet<String>) -> bool {
    let outer_re = outer_regex();
    let cand_caps = match outer_re.captures(candidate) {
        Some(c) => c,
        None => return false,
    };
    let cand_type = &cand_caps[1];
    let cand_inner = &cand_caps[2];
    // Replace wildcards with literal text so regex tests structural coverage
    let test_string = cand_inner.replace('*', "XXXPLACEHOLDERXXX");

    let allow_map = allow_regex_map();

    for existing in existing_set {
        if existing == candidate {
            continue;
        }
        let ex_caps = match outer_re.captures(existing) {
            Some(c) => c,
            None => continue,
        };
        if &ex_caps[1] != cand_type {
            continue;
        }
        // Hot path: reuse the precompiled regex when `existing` is a
        // FLOW universal allow entry — that covers the entire inner loop
        // of `merge_settings`. Cold path: compile per call for entries
        // sourced from the user's existing `settings.json`. The `.expect`
        // on the cold path mirrors the original contract: any entry that
        // captures with `outer_re` above is guaranteed to return `Some`
        // from `permission_to_regex` (same outer shape), so `.expect`
        // does not create an instrumented branch per
        // `.claude/rules/testability-means-simplicity.md`.
        let regex = match allow_map.get(existing.as_str()) {
            Some(r) => r.clone(),
            None => permission_to_regex(existing)
                .expect("outer_re match implies permission_to_regex succeeds"),
        };
        if regex.is_match(&test_string) {
            return true;
        }
    }
    false
}

/// Merge FLOW universal permissions into `.claude/settings.json`.
///
/// Additive merge — only adds entries not already present or subsumed
/// by broader patterns. Returns the merged settings dict as a JSON Value.
pub fn merge_settings(project_root: &Path) -> Result<Value, String> {
    let settings_dir = project_root.join(".claude");
    let settings_path = settings_dir.join("settings.json");

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Could not read settings.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Could not parse settings.json: {}", e))?
    } else {
        json!({})
    };

    // Ensure structure exists — guard against non-object top level
    if !settings.is_object() {
        settings = json!({});
    }
    if !matches!(settings.get("permissions"), Some(v) if v.is_object()) {
        settings["permissions"] = json!({});
    }
    if !matches!(settings["permissions"].get("allow"), Some(v) if v.is_array()) {
        settings["permissions"]["allow"] = json!([]);
    }
    if !matches!(settings["permissions"].get("deny"), Some(v) if v.is_array()) {
        settings["permissions"]["deny"] = json!([]);
    }

    // Additive merge — only add entries not already present or subsumed
    let mut existing_allow: HashSet<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mut allow_array: Vec<Value> = settings["permissions"]["allow"].as_array().unwrap().clone();

    for entry in UNIVERSAL_ALLOW {
        let e = entry.to_string();
        if !existing_allow.contains(&e) && !is_subsumed(&e, &existing_allow) {
            allow_array.push(Value::String(e.clone()));
            existing_allow.insert(e);
        }
    }

    let mut existing_deny: HashSet<String> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mut deny_array: Vec<Value> = settings["permissions"]["deny"].as_array().unwrap().clone();

    for entry in FLOW_DENY {
        let e = entry.to_string();
        if !existing_deny.contains(&e) {
            deny_array.push(Value::String(e.clone()));
            existing_deny.insert(e);
        }
    }

    // Always set defaultMode to acceptEdits
    let existing_mode = settings["permissions"]
        .get("defaultMode")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some(ref mode) = existing_mode {
        if mode != "acceptEdits" {
            eprintln!(
                "Warning: Overriding defaultMode '{}' with 'acceptEdits' — \
                 FLOW requires acceptEdits for state file writes",
                mode
            );
        }
    }

    settings["permissions"]["allow"] = Value::Array(allow_array);
    settings["permissions"]["deny"] = Value::Array(deny_array);
    settings["permissions"]["defaultMode"] = json!("acceptEdits");

    // Disable auto-backgrounding — CI gates must run in foreground to
    // enforce the gate. Without this, Claude Code may auto-background
    // long-running commands, letting the caller advance before CI finishes.
    if !matches!(settings.get("env"), Some(v) if v.is_object()) {
        settings["env"] = json!({});
    }
    settings["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"] = json!("false");

    // Write back
    fs::create_dir_all(&settings_dir)
        .map_err(|e| format!("Could not create .claude directory: {}", e))?;
    // `settings` is constructed from `json!` literals and merged-in
    // String/Array Values — serialization cannot fail in practice.
    // Surface any pathological internal error via `.expect` per
    // `.claude/rules/testability-means-simplicity.md`.
    let serialized = serde_json::to_string_pretty(&settings)
        .expect("serde_json::to_string_pretty on a built settings Value cannot fail");
    fs::write(&settings_path, format!("{}\n", serialized))
        .map_err(|e| format!("Could not write settings.json: {}", e))?;

    Ok(settings)
}

/// Write `.flow.json` with the plugin version and optional fields.
///
/// `.flow.json` is the per-project FLOW marker file. It is gitignored
/// and rewritten on every prime/upgrade. Consumers ignore unknown
/// fields, so older `.flow.json` files with extra keys continue to
/// parse cleanly during an in-place upgrade.
pub fn write_version_marker(
    project_root: &Path,
    version: &str,
    config_hash: Option<&str>,
    setup_hash: Option<&str>,
    commit_format: Option<&str>,
    plugin_root_path: Option<&str>,
    skills: Option<&Value>,
) -> Result<(), String> {
    let mut data = json!({
        "flow_version": version,
    });
    if let Some(h) = config_hash {
        data["config_hash"] = json!(h);
    }
    if let Some(h) = setup_hash {
        data["setup_hash"] = json!(h);
    }
    if let Some(f) = commit_format {
        data["commit_format"] = json!(f);
    }
    if let Some(p) = plugin_root_path {
        data["plugin_root"] = json!(p);
    }
    if let Some(s) = skills {
        data["skills"] = s.clone();
    }
    let flow_json = project_root.join(".flow.json");
    // `data` is constructed from `json!` literals and already-parsed
    // Value inputs, so serialization cannot fail — any error here
    // would indicate a serde_json internal bug, not a caller-visible
    // failure mode. Surface via `.expect` per
    // `.claude/rules/testability-means-simplicity.md`.
    let content =
        serde_json::to_string(&data).expect("serde_json::to_string on a built Value cannot fail");
    fs::write(&flow_json, format!("{}\n", content))
        .map_err(|e| format!("Could not write {}: {}", flow_json.display(), e))?;
    Ok(())
}

/// Add FLOW-specific entries to `.git/info/exclude` if not present.
///
/// Returns `true` if the file was updated, `false` if no changes needed.
pub fn update_git_exclude(project_root: &Path) -> bool {
    let output = match std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(project_root)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };

    let git_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // `Path::join` with an absolute argument returns the absolute
    // argument verbatim, and with a relative argument it roots the
    // argument at `project_root`. One join covers both cases the
    // prior if/else handled.
    let git_dir = project_root.join(&git_dir_str);

    let info_dir = git_dir.join("info");
    let _ = fs::create_dir_all(&info_dir);
    let exclude_path = info_dir.join("exclude");

    let mut content = if exclude_path.exists() {
        fs::read_to_string(&exclude_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut updated = false;
    for entry in EXCLUDE_ENTRIES {
        if !content.contains(entry) {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(entry);
            content.push('\n');
            updated = true;
        }
    }

    if updated {
        let _ = fs::write(&exclude_path, &content);
    }

    updated
}

/// Create a directory, write a script file, and make it executable (0o755).
///
/// The chmod after `fs::write` uses `fs::Permissions::from_mode(0o755)`
/// directly and surfaces any chmod failure as a panic via `.expect`.
/// On a POSIX filesystem we own, chmodding a file we just successfully
/// wrote is a local-filesystem invariant — any failure here indicates
/// an environmental corruption (network mount dropped, immutable
/// flag set by a privileged process) worth surfacing loudly rather
/// than swallowing as a Result::Err. The `.expect` does not create
/// an instrumented branch per
/// `.claude/rules/testability-means-simplicity.md`.
pub fn install_script(directory: &Path, filename: &str, content: &str) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|e| format!("Could not create directory {}: {}", directory.display(), e))?;
    let target = directory.join(filename);
    fs::write(&target, content)
        .map_err(|e| format!("Could not write {}: {}", target.display(), e))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
        .expect("chmod on newly-written file succeeds on a supported local filesystem");
    Ok(())
}

/// Install a pre-commit hook that blocks direct git commits during FLOW phases.
pub fn install_pre_commit_hook(project_root: &Path) -> Result<(), String> {
    install_script(
        &project_root.join(".git").join("hooks"),
        "pre-commit",
        PRE_COMMIT_HOOK,
    )
}

/// Resolve the user's home directory from `$HOME`. FLOW is
/// Unix-only (macOS/Linux); `HOME` is guaranteed to be set by the
/// shell, so the lookup is an invariant — any failure here is an
/// environmental corruption worth surfacing via `.expect()`.
fn home_dir() -> PathBuf {
    PathBuf::from(
        env::var("HOME").expect("HOME environment variable is set on all supported platforms"),
    )
}

/// Install a global flow launcher at `~/.local/bin/flow`.
pub fn install_launcher(home: &Path) -> Result<(), String> {
    install_script(&home.join(".local").join("bin"), "flow", LAUNCHER_SCRIPT)
}

/// Report whether `~/.local/bin` is in PATH and print the
/// corresponding status line to stderr.
///
/// Emits a one-line stderr message describing the current state —
/// either the "add to PATH" warning when missing, or a confirming
/// "already in PATH" line when present. Branchless selection via
/// array indexing so both states are exercised by a single
/// always-run call path.
pub fn check_launcher_path(home: &Path) {
    let local_bin = home.join(".local").join("bin");
    let local_bin_str = local_bin.to_string_lossy().to_string();
    let path_var = env::var("PATH").unwrap_or_default();
    let dirs: Vec<&str> = path_var.split(':').collect();
    let in_path = dirs.contains(&local_bin_str.as_str());
    let messages = [
        format!(
            "Warning: {} is not in your PATH. \
             Add this to your shell profile:\n  \
             export PATH=\"$HOME/.local/bin:$PATH\"",
            local_bin_str
        ),
        format!(
            "Launcher directory {} is already in your PATH.",
            local_bin_str
        ),
    ];
    eprintln!("{}", messages[in_path as usize]);
}

#[derive(ClapArgs)]
pub struct Args {
    /// Project root directory
    pub project_root: String,

    /// JSON string of skills configuration
    #[arg(long = "skills-json")]
    pub skills_json: Option<String>,

    /// Commit message format (full or title-only)
    #[arg(long = "commit-format")]
    pub commit_format: Option<String>,

    /// Plugin root path for launcher installation
    #[arg(long = "plugin-root")]
    pub plugin_root: Option<String>,
}

/// Run the prime-setup sequence.
///
/// Returns `Err(Value)` for all error cases (printed as JSON, exit 1).
/// `Ok(Value)` for success.
///
/// Writes universal permissions to `.claude/settings.json`, writes
/// the version marker to `.flow.json`, updates `.git/info/exclude`,
/// installs the pre-commit hook and global launcher, and copies the
/// `bin/*` stubs from `assets/bin-stubs/<tool>.sh` into
/// `<project_root>/bin/<tool>` when absent. Pre-existing user `bin/*`
/// files are never overwritten so users who already configured their
/// own toolchain keep their work.
pub fn run_impl(args: &Args) -> Result<Value, Value> {
    let project_root = PathBuf::from(&args.project_root);
    if !project_root.is_dir() {
        return Err(json!({
            "status": "error",
            "message": format!("Project root not found: {}", args.project_root),
        }));
    }

    let skills: Option<Value> = match &args.skills_json {
        Some(s) => match serde_json::from_str(s) {
            Ok(v) => Some(v),
            Err(e) => {
                return Err(json!({
                    "status": "error",
                    "message": format!("Invalid --skills-json: {}", e),
                }));
            }
        },
        None => None,
    };

    let p_root = match plugin_root() {
        Some(p) => p,
        None => {
            return Err(json!({
                "status": "error",
                "message": "Plugin root not found",
            }));
        }
    };

    let version = read_version();
    if version == "?" {
        return Err(json!({
            "status": "error",
            "message": "Could not read plugin version",
        }));
    }

    let config_hash = compute_config_hash();
    // compute_setup_hash reads <plugin_root>/src/prime_setup.rs. On a
    // well-formed plugin install this always succeeds. If it fails
    // (corrupt install, missing source file), fall back to an empty
    // hash — prime_check will compare against stored hashes and
    // trigger a re-prime on any mismatch, so the worst case is an
    // extra prime run, not a user-visible error.
    let setup_hash = compute_setup_hash(&p_root).unwrap_or_default();

    merge_settings(&project_root).map_err(|e| json!({"status": "error", "message": e}))?;

    write_version_marker(
        &project_root,
        &version,
        Some(&config_hash),
        Some(&setup_hash),
        args.commit_format.as_deref(),
        args.plugin_root.as_deref(),
        skills.as_ref(),
    )
    .map_err(|e| json!({"status": "error", "message": e}))?;

    let exclude_updated = update_git_exclude(&project_root);

    install_pre_commit_hook(&project_root).map_err(|e| json!({"status": "error", "message": e}))?;

    let mut launcher_installed = false;
    if args.plugin_root.is_some() {
        let home = home_dir();
        if let Err(e) = install_launcher(&home) {
            eprintln!("Warning: Could not install launcher: {}", e);
        } else {
            check_launcher_path(&home);
            launcher_installed = true;
        }
    }

    // bin/* stub installer copies assets/bin-stubs/<tool>.sh into
    // <project_root>/bin/<tool> for any of [format, lint, build, test]
    // that does not already exist. Pre-existing files are never
    // overwritten so users who already configured their own bin/* keep
    // their work.
    let stubs_installed = install_bin_stubs(&project_root, &p_root);

    Ok(json!({
        "status": "ok",
        "settings_merged": true,
        "exclude_updated": exclude_updated,
        "version_marker": true,
        "hook_installed": true,
        "launcher_installed": launcher_installed,
        "stubs_installed": stubs_installed,
    }))
}

/// Install the four FLOW bin/* stubs into `<project_root>/bin/` when absent.
///
/// Reads each `assets/bin-stubs/<tool>.sh` from the plugin root, writes
/// it to `<project_root>/bin/<tool>` (creating `bin/` if needed), and
/// chmods 0o755. Pre-existing files are never overwritten — the stub
/// installer only fills in the gaps so users who already configured
/// their own bin/* scripts (or migrated by hand) keep their work.
///
/// # Symlink safety
///
/// The existence check uses [`fs::symlink_metadata`], which does not
/// follow symlinks. This matters because `Path::exists()` would return
/// `false` for a dangling symlink, and a subsequent `fs::write` would
/// then follow the symlink and write to its target — potentially
/// anywhere on the filesystem the user has write permission. Using
/// `symlink_metadata` correctly detects the symlink entry itself and
/// skips it, so the installer never writes through a symlink.
///
/// Returns the list of tool names that were actually installed.
pub fn install_bin_stubs(project_root: &Path, plugin_root: &Path) -> Vec<String> {
    let stubs_dir = plugin_root.join("assets").join("bin-stubs");
    let bin_dir = project_root.join("bin");
    let mut installed = Vec::new();
    for tool in ["format", "lint", "build", "test"] {
        let target = bin_dir.join(tool);
        // symlink_metadata returns Ok for files, directories, valid
        // symlinks, and dangling symlinks — anything the filesystem
        // considers an entry. This is the only safe existence check
        // for a path we are about to write to.
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let source = stubs_dir.join(format!("{}.sh", tool));
        if !source.exists() {
            continue;
        }
        if fs::create_dir_all(&bin_dir).is_err() {
            continue;
        }
        let content = match fs::read_to_string(&source) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if fs::write(&target, &content).is_err() {
            continue;
        }
        // `fs::write` just created the file; chmodding it to 0o755 is
        // a local-filesystem invariant and cannot legitimately fail.
        // Swallow any pathological error (network mount dropped,
        // immutable flag set by a privileged process) — the installer
        // still records the tool as installed because the file bytes
        // are on disk.
        let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
        installed.push(tool.to_string());
    }
    installed
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match run_impl(args) {
        Ok(value) => (value, 0),
        Err(value) => (value, 1),
    }
}
