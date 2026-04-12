//! Shared test helpers for FLOW integration tests.
//!
//! Provides path resolution, file reading, phase/skill enumeration
//! (used by structural, contract, permission, docs-sync tests),
//! and start_* test helpers (git repo setup, flow.json, gh stubs).

// Not every consumer uses every helper. Each test file imports only what it needs.
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flow_rs::flow_paths::FlowPaths;
use serde_json::{json, Value};

// --- Path helpers ---

/// Returns the repository root (CARGO_MANIFEST_DIR at compile time).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Returns the skills/ directory path.
pub fn skills_dir() -> PathBuf {
    repo_root().join("skills")
}

/// Returns the docs/ directory path.
pub fn docs_dir() -> PathBuf {
    repo_root().join("docs")
}

/// Returns the hooks/ directory path.
pub fn hooks_dir() -> PathBuf {
    repo_root().join("hooks")
}

/// Returns the bin/ directory path.
pub fn bin_dir() -> PathBuf {
    repo_root().join("bin")
}

/// Returns the agents/ directory path.
pub fn agents_dir() -> PathBuf {
    repo_root().join("agents")
}

// --- FlowPaths test fixtures ---

/// Returns the `.flow-states/` directory under `project_root` without
/// creating it. Equivalent to `FlowPaths::new(project_root, "").flow_states_dir()`
/// but shorter at callsites — test fixtures use this in place of the
/// old `dir.path().join(".flow-states")` literal so the directory
/// name stays owned by `FlowPaths`.
pub fn flow_states_dir(project_root: &Path) -> std::path::PathBuf {
    FlowPaths::new(project_root, "").flow_states_dir()
}

// --- File reading helpers ---

/// Reads and returns the content of `skills/{name}/SKILL.md`.
pub fn read_skill(name: &str) -> String {
    let path = skills_dir().join(name).join("SKILL.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
}

/// Reads and parses `flow-phases.json` from the repo root.
pub fn load_phases() -> Value {
    let path = repo_root().join("flow-phases.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
}

/// Returns the plugin version from `.claude-plugin/plugin.json`.
pub fn plugin_version() -> String {
    let path = repo_root().join(".claude-plugin").join("plugin.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    let parsed: Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e));
    parsed["version"]
        .as_str()
        .expect("plugin.json missing 'version' key")
        .to_string()
}

/// Read current plugin version from .claude-plugin/plugin.json.
/// Alias for plugin_version() — used by start_* tests.
pub fn current_plugin_version() -> String {
    plugin_version()
}

// --- Skill/phase enumeration ---

/// Returns sorted list of all skill directory names under `skills/`.
pub fn all_skill_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(skills_dir())
        .expect("Failed to read skills/ directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Returns the ordered phase keys from flow-phases.json `order` array.
pub fn phase_order() -> Vec<String> {
    let phases = load_phases();
    phases["order"]
        .as_array()
        .expect("flow-phases.json missing 'order' array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// Returns `(phase_key, skill_name)` pairs for all phases.
pub fn phase_skills() -> Vec<(String, String)> {
    phase_order()
        .into_iter()
        .map(|key| {
            let skill_name = key.clone();
            (key, skill_name)
        })
        .collect()
}

/// Returns sorted list of skill names that are NOT phases.
pub fn utility_skills() -> Vec<String> {
    let phase_keys: Vec<String> = phase_order();
    let mut utils: Vec<String> = all_skill_names()
        .into_iter()
        .filter(|name| !phase_keys.contains(name))
        .collect();
    utils.sort();
    utils
}

/// Reads and returns the content of an agent file at `agents/{name}`.
pub fn read_agent(name: &str) -> String {
    let path = agents_dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
}

/// Reads and parses `hooks/hooks.json` from the repo root.
pub fn load_hooks() -> Value {
    let path = hooks_dir().join("hooks.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
}

/// Reads `.claude/settings.json` from the repo root.
pub fn load_settings() -> Value {
    let path = repo_root().join(".claude").join("settings.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
}

// --- Markdown file collection ---

/// Collects all `.md` files recursively under a directory.
pub fn collect_md_files(dir: &PathBuf) -> Vec<(String, String)> {
    let mut results = Vec::new();
    collect_md_files_recursive(dir, dir, &mut results);
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

fn collect_md_files_recursive(
    base: &PathBuf,
    current: &PathBuf,
    results: &mut Vec<(String, String)>,
) {
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_md_files_recursive(base, &path, results);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(base)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    results.push((rel, content));
                }
            }
        }
    }
}

/// Extracts all fenced bash blocks from markdown content.
pub fn extract_bash_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_block = String::new();

    for line in content.lines() {
        if line.trim_start().starts_with("```bash") && !in_block {
            in_block = true;
            current_block.clear();
        } else if line.trim_start().starts_with("```") && in_block {
            in_block = false;
            if !current_block.is_empty() {
                blocks.push(current_block.trim().to_string());
            }
        } else if in_block {
            let stripped = if let Some(s) = line.strip_prefix("> ") {
                s
            } else {
                line
            };
            current_block.push_str(stripped);
            current_block.push('\n');
        }
    }

    blocks
}

// --- Start test helpers (from main branch) ---

/// Create a bare+clone git repo pair for testing.
pub fn create_git_repo_with_remote(parent: &Path) -> PathBuf {
    let bare = parent.join("bare.git");
    let repo = parent.join("repo");

    Command::new("git")
        .args(["init", "--bare", "-b", "main", &bare.to_string_lossy()])
        .output()
        .unwrap();

    Command::new("git")
        .args(["clone", &bare.to_string_lossy(), &repo.to_string_lossy()])
        .output()
        .unwrap();

    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&repo)
            .output()
            .unwrap();
    }

    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    Command::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    repo
}

/// Write .flow.json with version and optional skills config.
///
/// `prime_setup` writes the file with these two keys (plus hashes,
/// commit_format, and plugin_root when provided). Older callers that
/// passed a positional language name should drop the argument.
pub fn write_flow_json(repo: &Path, version: &str, skills: Option<&Value>) {
    let mut data = json!({
        "flow_version": version,
    });
    if let Some(sk) = skills {
        data["skills"] = sk.clone();
    }
    fs::write(repo.join(".flow.json"), data.to_string()).unwrap();
}

/// Create a custom gh stub script. Returns the stub directory.
pub fn create_gh_stub(repo: &Path, script: &str) -> PathBuf {
    let stub_dir = repo.join(".stub-bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    fs::write(&gh_stub, script).unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
    stub_dir
}

/// Parse JSON from the last line of stdout. Uses last-line extraction to
/// filter out child process output (git messages, etc.) that precedes the JSON.
pub fn parse_output(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last_line).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
}
