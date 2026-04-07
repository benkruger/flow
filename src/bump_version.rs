//! Port of lib/bump-version.py — bump FLOW plugin version across all files.
//!
//! Updates plugin.json, marketplace.json, and all skill banners.
//!
//! Usage:
//!   bin/flow bump-version <new_version>
//!
//! Output (human-readable to stdout):
//!   Success: "Bumped X.Y.Z -> A.B.C\nUpdated N files:\n  ..."
//!   Error:   "Error: ..." (exit 1)

use std::fs;
use std::path::Path;

use clap::Parser;
use regex::Regex;

use crate::utils::plugin_root;

#[derive(Parser, Debug)]
#[command(name = "bump-version", about = "Bump FLOW plugin version")]
pub struct Args {
    /// New version (semver: X.Y.Z)
    pub version: Option<String>,
}

/// Validate that a version string matches `X.Y.Z` format.
pub fn validate_version(version: &str) -> bool {
    let re = Regex::new(r"^\d+\.\d+\.\d+$").unwrap();
    re.is_match(version)
}

/// Read the current version from plugin.json.
pub fn read_current_version(plugin_json: &Path) -> Result<String, String> {
    let text = fs::read_to_string(plugin_json)
        .map_err(|e| format!("Failed to read {}: {}", plugin_json.display(), e))?;
    let data: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON in {}: {}", plugin_json.display(), e))?;
    data["version"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("No \"version\" field in {}", plugin_json.display()))
}

/// Replace `"version": "old"` with `"version": "new"` in a JSON file.
/// Returns true if any replacement was made.
pub fn bump_json(path: &Path, old: &str, new: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let old_pattern = format!("\"version\": \"{}\"", old);
    let new_pattern = format!("\"version\": \"{}\"", new);
    let updated = text.replace(&old_pattern, &new_pattern);
    if updated == text {
        return Ok(false);
    }
    fs::write(path, &updated).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(true)
}

/// Replace `FLOW vOLD` with `FLOW vNEW` in a skill file.
/// Returns true if any replacement was made.
pub fn bump_skill(path: &Path, old: &str, new: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let old_pattern = format!("FLOW v{}", old);
    let new_pattern = format!("FLOW v{}", new);
    let updated = text.replace(&old_pattern, &new_pattern);
    if updated == text {
        return Ok(false);
    }
    fs::write(path, &updated).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(true)
}

/// Orchestrate the full version bump across all files.
///
/// Returns Ok(summary_text) on success, Err(error_text) on failure.
/// The caller (run) prints the result and exits accordingly.
pub fn run_impl(args: &Args, repo_root: &Path) -> Result<String, String> {
    let new_version = match &args.version {
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

    // 1. plugin.json
    if bump_json(&plugin_json, &old_version, new_version)? {
        changed.push(
            plugin_json
                .strip_prefix(repo_root)
                .unwrap_or(&plugin_json)
                .to_string_lossy()
                .to_string(),
        );
    }

    // 2. marketplace.json
    let marketplace_json = repo_root.join(".claude-plugin").join("marketplace.json");
    if marketplace_json.exists() && bump_json(&marketplace_json, &old_version, new_version)? {
        changed.push(
            marketplace_json
                .strip_prefix(repo_root)
                .unwrap_or(&marketplace_json)
                .to_string_lossy()
                .to_string(),
        );
    }

    // 3. skills/*/SKILL.md — filter dot-prefixed entries per rust-port-parity rule
    let skills_dir = repo_root.join("skills");
    if skills_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(&skills_dir)
            .map_err(|e| format!("Failed to read skills dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !name.starts_with('.') && e.path().join("SKILL.md").exists()
            })
            .collect();
        // Sort for deterministic output matching Python's sorted()
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let skill_file = entry.path().join("SKILL.md");
            if bump_skill(&skill_file, &old_version, new_version)? {
                changed.push(
                    skill_file
                        .strip_prefix(repo_root)
                        .unwrap_or(&skill_file)
                        .to_string_lossy()
                        .to_string(),
                );
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
        changed.push(
            release_skill
                .strip_prefix(repo_root)
                .unwrap_or(&release_skill)
                .to_string_lossy()
                .to_string(),
        );
    }

    let mut output = format!("Bumped {} -> {}\n", old_version, new_version);
    output.push_str(&format!("Updated {} files:\n", changed.len()));
    for f in &changed {
        output.push_str(&format!("  {}\n", f));
    }

    Ok(output.trim_end().to_string())
}

pub fn run(args: Args) {
    let repo_root = match plugin_root() {
        Some(r) => r,
        None => {
            eprintln!("Error: could not find FLOW plugin root");
            std::process::exit(1);
        }
    };

    match run_impl(&args, &repo_root) {
        Ok(output) => {
            println!("{}", output);
        }
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}
