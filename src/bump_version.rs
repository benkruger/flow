//! Bump FLOW plugin version across all files.
//!
//! Updates plugin.json, marketplace.json, and all skill banners.
//!
//! Usage:
//!   bin/flow bump-version <new_version>
//!
//! Output (human-readable to stdout):
//!   Success: "Bumped X.Y.Z -> A.B.C\nUpdated N files:\n  ..."
//!   Error:   "Error: ..." (exit 1)
//!
//! Tests live at `tests/bump_version.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::fs;
use std::path::Path;

/// Validate that a version string matches `X.Y.Z` semver format.
pub fn validate_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    for p in &parts {
        if p.is_empty() {
            return false;
        }
        for c in p.chars() {
            if !c.is_ascii_digit() {
                return false;
            }
        }
    }
    true
}

/// Read the current version from plugin.json.
pub fn read_current_version(plugin_json: &Path) -> Result<String, String> {
    let text = match fs::read_to_string(plugin_json) {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to read {}: {}", plugin_json.display(), e)),
    };
    let data: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Err(format!("Invalid JSON in {}: {}", plugin_json.display(), e)),
    };
    match data["version"].as_str() {
        Some(s) => Ok(s.to_string()),
        None => Err(format!("No \"version\" field in {}", plugin_json.display())),
    }
}

/// Replace `"version": "old"` with `"version": "new"` in a JSON file.
/// Returns true if any replacement was made.
pub fn bump_json(path: &Path, old: &str, new: &str) -> Result<bool, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };
    let old_pattern = format!("\"version\": \"{}\"", old);
    let new_pattern = format!("\"version\": \"{}\"", new);
    let updated = text.replace(&old_pattern, &new_pattern);
    if updated == text {
        return Ok(false);
    }
    if let Err(e) = fs::write(path, &updated) {
        return Err(format!("Failed to write {}: {}", path.display(), e));
    }
    Ok(true)
}

/// Replace `FLOW vOLD` with `FLOW vNEW` in a skill file.
/// Returns true if any replacement was made.
pub fn bump_skill(path: &Path, old: &str, new: &str) -> Result<bool, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };
    let old_pattern = format!("FLOW v{}", old);
    let new_pattern = format!("FLOW v{}", new);
    let updated = text.replace(&old_pattern, &new_pattern);
    if updated == text {
        return Ok(false);
    }
    if let Err(e) = fs::write(path, &updated) {
        return Err(format!("Failed to write {}: {}", path.display(), e));
    }
    Ok(true)
}

/// Orchestrate the full version bump across all files.
///
/// Returns Ok(summary_text) on success, Err(error_text) on failure.
/// The caller (run) prints the result and exits accordingly.
pub fn run_impl(version: Option<&str>, repo_root: &Path) -> Result<String, String> {
    let new_version = match version {
        Some(v) => v,
        None => return Err("Usage: bin/flow bump-version <new_version>".to_string()),
    };

    if !validate_version(new_version) {
        return Err(format!("Error: invalid version format: {}", new_version));
    }

    let plugin_json = repo_root.join(".claude-plugin").join("plugin.json");
    if !plugin_json.exists() {
        return Err(format!("Error: {} not found", plugin_json.display()));
    }

    let old_version = read_current_version(&plugin_json)?;
    if old_version == *new_version {
        return Err(format!("Error: version is already {}", new_version));
    }

    let mut changed: Vec<String> = Vec::new();

    // 1. plugin.json — always bumps (old_version was just read from it).
    bump_json(&plugin_json, &old_version, new_version)?;
    changed.push(".claude-plugin/plugin.json".to_string());

    // 2. marketplace.json
    let marketplace_json = repo_root.join(".claude-plugin").join("marketplace.json");
    if marketplace_json.exists() && bump_json(&marketplace_json, &old_version, new_version)? {
        changed.push(".claude-plugin/marketplace.json".to_string());
    }

    // 3. skills/*/SKILL.md — filter dot-prefixed entries (fnmatch convention)
    let skills_dir = repo_root.join("skills");
    if skills_dir.exists() {
        let read_dir = match fs::read_dir(&skills_dir) {
            Ok(rd) => rd,
            Err(e) => return Err(format!("Failed to read skills dir: {}", e)),
        };
        let mut entries: Vec<_> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !name.starts_with('.') && e.path().join("SKILL.md").exists()
            })
            .collect();
        // Sort by file name so the bump output is byte-stable across
        // runs and machines. Without this, version-bump diffs would
        // shuffle skill order based on filesystem iteration order.
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let skill_file = entry.path().join("SKILL.md");
            if bump_skill(&skill_file, &old_version, new_version)? {
                let name = entry.file_name();
                changed.push(format!("skills/{}/SKILL.md", name.to_string_lossy()));
            }
        }
    }

    // 4. .claude/skills/flow-release/SKILL.md
    let release_skill = repo_root
        .join(".claude")
        .join("skills")
        .join("flow-release")
        .join("SKILL.md");
    if release_skill.exists() && bump_skill(&release_skill, &old_version, new_version)? {
        changed.push(".claude/skills/flow-release/SKILL.md".to_string());
    }

    let mut output = format!("Bumped {} -> {}\n", old_version, new_version);
    output.push_str(&format!("Updated {} files:\n", changed.len()));
    for f in &changed {
        output.push_str(&format!("  {}\n", f));
    }

    Ok(output.trim_end().to_string())
}

/// Dispatch from a resolved `plugin_root` option to `(message, code)`.
/// Main-arm calls this with `plugin_root()` and dispatches the text.
/// Tests call it with `Some(tempdir)` or `None` directly — no separate
/// closure seam.
pub fn run_impl_main(
    version: Option<&str>,
    plugin_root: Option<std::path::PathBuf>,
) -> (String, i32) {
    let repo_root = match plugin_root {
        Some(r) => r,
        None => return ("Error: could not find FLOW plugin root".to_string(), 1),
    };
    match run_impl(version, &repo_root) {
        Ok(output) => (output, 0),
        Err(e) => (e, 1),
    }
}
