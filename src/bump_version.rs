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
    let data: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Invalid JSON in {}: {}", plugin_json.display(), e))?;
    data["version"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("No \"version\" field in {}", plugin_json.display()))
}

/// Replace `"version": "old"` with `"version": "new"` in a JSON file.
/// Returns true if any replacement was made.
pub fn bump_json(path: &Path, old: &str, new: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
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
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
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

    // 3. skills/*/SKILL.md — filter dot-prefixed entries (fnmatch convention)
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
        // Sort by file name so the bump output is byte-stable across
        // runs and machines. Without this, version-bump diffs would
        // shuffle skill order based on filesystem iteration order.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup_repo(version: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plugin_dir = root.join(".claude-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.json"),
            format!("{{\"name\": \"flow\", \"version\": \"{}\"}}", version),
        )
        .unwrap();
        (dir, root)
    }

    #[test]
    fn validate_version_semver() {
        assert!(validate_version("1.0.0"));
        assert!(validate_version("10.20.30"));
        assert!(!validate_version("1.0"));
        assert!(!validate_version("1.0.0-rc1"));
        assert!(!validate_version("v1.0.0"));
        assert!(!validate_version(""));
    }

    #[test]
    fn read_current_version_reads_plugin_json() {
        let (_dir, root) = setup_repo("1.2.3");
        let version =
            read_current_version(&root.join(".claude-plugin").join("plugin.json")).unwrap();
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn read_current_version_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_current_version(&dir.path().join("nonexistent.json")).unwrap_err();
        assert!(err.contains("Failed to read"));
    }

    #[test]
    fn read_current_version_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, "not json").unwrap();
        let err = read_current_version(&path).unwrap_err();
        assert!(err.contains("Invalid JSON"));
    }

    #[test]
    fn read_current_version_missing_version_field_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, r#"{"name": "flow"}"#).unwrap();
        let err = read_current_version(&path).unwrap_err();
        assert!(err.contains("No \"version\" field"));
    }

    #[test]
    fn bump_json_replaces_version_string() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, r#"{"version": "1.0.0", "name": "flow"}"#).unwrap();
        let changed = bump_json(&path, "1.0.0", "2.0.0").unwrap();
        assert!(changed);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"version\": \"2.0.0\""));
    }

    #[test]
    fn bump_json_no_change_when_version_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, r#"{"version": "1.0.0"}"#).unwrap();
        let changed = bump_json(&path, "9.9.9", "2.0.0").unwrap();
        assert!(!changed);
    }

    #[test]
    fn bump_skill_replaces_banner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "FLOW v1.0.0 — Start\nbody").unwrap();
        let changed = bump_skill(&path, "1.0.0", "2.0.0").unwrap();
        assert!(changed);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("FLOW v2.0.0"));
    }

    #[test]
    fn bump_skill_no_change_when_banner_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "no banner here").unwrap();
        let changed = bump_skill(&path, "1.0.0", "2.0.0").unwrap();
        assert!(!changed);
    }

    #[test]
    fn run_impl_missing_version_arg_errors() {
        let (_dir, root) = setup_repo("1.0.0");
        let args = Args { version: None };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(err.contains("Usage:"));
    }

    #[test]
    fn run_impl_invalid_version_format_errors() {
        let (_dir, root) = setup_repo("1.0.0");
        let args = Args {
            version: Some("not-a-version".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(err.contains("invalid version format"));
    }

    #[test]
    fn run_impl_missing_plugin_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            version: Some("2.0.0".to_string()),
        };
        let err = run_impl(&args, dir.path()).unwrap_err();
        assert!(err.contains("plugin.json"));
        assert!(err.contains("not found"));
    }

    #[test]
    fn run_impl_same_version_errors() {
        let (_dir, root) = setup_repo("1.0.0");
        let args = Args {
            version: Some("1.0.0".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(err.contains("already 1.0.0"));
    }

    #[test]
    fn run_impl_bumps_plugin_json_and_reports() {
        let (_dir, root) = setup_repo("1.0.0");
        let args = Args {
            version: Some("2.0.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("Bumped 1.0.0 -> 2.0.0"));
        assert!(output.contains("plugin.json"));
        let contents = fs::read_to_string(root.join(".claude-plugin").join("plugin.json")).unwrap();
        assert!(contents.contains("\"version\": \"2.0.0\""));
    }

    #[test]
    fn run_impl_bumps_marketplace_json_when_present() {
        let (_dir, root) = setup_repo("1.0.0");
        let marketplace = root.join(".claude-plugin").join("marketplace.json");
        fs::write(&marketplace, r#"{"version": "1.0.0"}"#).unwrap();
        let args = Args {
            version: Some("2.0.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("marketplace.json"));
    }

    #[test]
    fn run_impl_bumps_skill_banners_sorted_by_name() {
        let (_dir, root) = setup_repo("1.0.0");
        let skills_dir = root.join("skills");
        for name in ["z-skill", "a-skill"] {
            let skill_dir = skills_dir.join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(skill_dir.join("SKILL.md"), "FLOW v1.0.0 — Test\n").unwrap();
        }
        let hidden = skills_dir.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(hidden.join("SKILL.md"), "FLOW v1.0.0 — Hidden\n").unwrap();

        let args = Args {
            version: Some("2.0.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("a-skill"));
        assert!(output.contains("z-skill"));
        assert!(!output.contains(".hidden"));
        let hidden_content = fs::read_to_string(hidden.join("SKILL.md")).unwrap();
        assert!(hidden_content.contains("FLOW v1.0.0"));
    }

    #[test]
    fn run_impl_bumps_flow_release_skill_when_present() {
        let (_dir, root) = setup_repo("1.0.0");
        let release_dir = root.join(".claude").join("skills").join("flow-release");
        fs::create_dir_all(&release_dir).unwrap();
        fs::write(release_dir.join("SKILL.md"), "FLOW v1.0.0 — Release\n").unwrap();
        let args = Args {
            version: Some("2.0.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("flow-release"));
    }
}
