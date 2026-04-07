//! Shared test helpers for FLOW integration tests.
//!
//! Provides path resolution, file reading, and phase/skill enumeration
//! used across structural, contract, permission, and docs-sync tests.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

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

/// Returns the frameworks/ directory path.
pub fn frameworks_dir() -> PathBuf {
    repo_root().join("frameworks")
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

/// Reads and returns the content of `skills/{name}/SKILL.md`.
///
/// Panics if the file does not exist or cannot be read.
pub fn read_skill(name: &str) -> String {
    let path = skills_dir().join(name).join("SKILL.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
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
///
/// The skill name is derived from the phase key (e.g. "flow-start" → "flow-start").
/// Phase keys are in canonical order from flow-phases.json.
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
///
/// Panics if the file does not exist or cannot be read.
pub fn read_agent(name: &str) -> String {
    let path = agents_dir().join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
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

/// Collects all `.md` files recursively under a directory.
///
/// Returns `(relative_path, content)` pairs where relative_path is
/// relative to the given base directory.
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
///
/// Returns the content inside each ```bash ... ``` block.
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
            // Strip blockquote markers
            let stripped = if line.starts_with("> ") {
                &line[2..]
            } else {
                line
            };
            current_block.push_str(stripped);
            current_block.push('\n');
        }
    }

    blocks
}
