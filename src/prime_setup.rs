//! Consolidated setup for FLOW Prime.
//!
//! Merges permissions into
//! `.claude/settings.json`, writes `.flow.json` version marker,
//! updates `.git/info/exclude`, installs hooks and launcher.
//! Does NOT commit — the skill handles `git add` + `commit`.
//!
//! Usage: `bin/flow prime-setup <project_root> --framework <name>`
//!
//! Output (JSON to stdout):
//!   Success: `{"status": "ok", "settings_merged": true, ...}`
//!   Failure: `{"status": "error", "message": "..."}`

use std::collections::HashSet;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;

use clap::Args as ClapArgs;
use regex::Regex;
use serde_json::{json, Value};

use crate::create_dependencies;
use crate::prime_check::{
    compute_config_hash, compute_setup_hash, load_framework_permissions, EXCLUDE_ENTRIES,
    FLOW_DENY, UNIVERSAL_ALLOW,
};
use crate::prime_project;
use crate::utils::{frameworks_dir, permission_to_regex, plugin_root, read_version};

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

/// Resolve derived permissions from `frameworks/<name>/permissions.json`.
///
/// Reads the optional `derived_permissions` array. Each entry has a glob
/// pattern and a template with a `{stem}` placeholder. The glob is
/// matched against the project root (skipping dot-prefixed entries per
/// the fnmatch convention where `*` does not match leading dots), and
/// `{stem}` is replaced with the matched path's stem.
///
/// Returns an empty vec if no derived permissions are configured or matched.
pub fn derive_permissions(project_root: &Path, framework: &str, fw_dir: &Path) -> Vec<String> {
    let permissions_path = fw_dir.join(framework).join("permissions.json");
    if !permissions_path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(&permissions_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let data: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let derived = match data.get("derived_permissions").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut results = Vec::new();
    for entry in derived {
        let glob_pattern = match entry.get("glob").and_then(|v| v.as_str()) {
            Some(g) => g,
            None => continue,
        };
        let template = match entry.get("template").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };
        // Extract the suffix from the glob pattern (e.g. "*.xcodeproj" -> ".xcodeproj")
        let suffix = if let Some(s) = glob_pattern.strip_prefix('*') {
            s
        } else {
            continue;
        };

        // Read directory entries, filter out dot-prefixed names
        // (fnmatch convention — `*` does not match leading dots),
        // match the suffix, sort for determinism, and take the first
        // match only.
        let entries = match fs::read_dir(project_root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut matches: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                // Skip dot-prefixed entries — `*.ext` follows the
                // fnmatch convention where `*` does not match leading
                // dots, so a stray `.local.xcodeproj` does not match.
                if !name.starts_with('.') {
                    // strip_suffix is UTF-8 safe — no byte-index arithmetic
                    name.strip_suffix(suffix).map(|stem| stem.to_string())
                } else {
                    None
                }
            })
            .collect();
        matches.sort();
        if let Some(stem) = matches.first() {
            results.push(template.replace("{stem}", stem));
            // Only the first match per entry is used — derived
            // permissions are 1:1 with their glob pattern, so a second
            // match would produce a duplicate permission entry.
        }
    }
    results
}

/// Check if any entry in `existing_set` pattern-subsumes `candidate`.
///
/// Uses `permission_to_regex()` to test whether an existing broader pattern
/// (e.g. `Agent(*)`) matches the candidate's concrete form (e.g.
/// `Agent(flow:ci-fixer)`). Only checks same-type entries (e.g. Agent vs
/// Agent, Read vs Read); never matches across types (Agent vs Bash).
pub fn is_subsumed(candidate: &str, existing_set: &HashSet<String>) -> bool {
    let outer_re = Regex::new(r"^(\w+)\((.+)\)$").unwrap();
    let cand_caps = match outer_re.captures(candidate) {
        Some(c) => c,
        None => return false,
    };
    let cand_type = &cand_caps[1];
    let cand_inner = &cand_caps[2];
    // Replace wildcards with literal text so regex tests structural coverage
    let test_string = cand_inner.replace('*', "XXXPLACEHOLDERXXX");

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
        if let Some(regex) = permission_to_regex(existing) {
            if regex.is_match(&test_string) {
                return true;
            }
        }
    }
    false
}

/// Build the merged allow list for the given framework.
fn allow_list(framework: &str, fw_dir: &Path) -> Vec<String> {
    let mut list: Vec<String> = UNIVERSAL_ALLOW.iter().map(|s| s.to_string()).collect();
    list.extend(load_framework_permissions(framework, fw_dir));
    list
}

/// Merge FLOW permissions into `.claude/settings.json`.
///
/// Additive merge — only adds entries not already present or subsumed
/// by broader patterns. Returns the merged settings dict as a JSON Value.
pub fn merge_settings(
    project_root: &Path,
    framework: &str,
    fw_dir: &Path,
) -> Result<Value, String> {
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

    for entry in allow_list(framework, fw_dir) {
        if !existing_allow.contains(&entry) && !is_subsumed(&entry, &existing_allow) {
            allow_array.push(Value::String(entry.clone()));
            existing_allow.insert(entry);
        }
    }

    // Merge derived permissions
    for entry in derive_permissions(project_root, framework, fw_dir) {
        if !existing_allow.contains(&entry) && !is_subsumed(&entry, &existing_allow) {
            allow_array.push(Value::String(entry.clone()));
            existing_allow.insert(entry);
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
    let serialized = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Could not serialize settings: {}", e))?;
    fs::write(&settings_path, format!("{}\n", serialized))
        .map_err(|e| format!("Could not write settings.json: {}", e))?;

    Ok(settings)
}

/// Write `.flow.json` with the plugin version, framework, and optional fields.
#[allow(clippy::too_many_arguments)]
pub fn write_version_marker(
    project_root: &Path,
    version: &str,
    framework: &str,
    config_hash: Option<&str>,
    setup_hash: Option<&str>,
    commit_format: Option<&str>,
    plugin_root_path: Option<&str>,
    skills: Option<&Value>,
) -> Result<(), String> {
    let mut data = json!({
        "flow_version": version,
        "framework": framework,
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
    let content = serde_json::to_string(&data)
        .map_err(|e| format!("Could not serialize .flow.json: {}", e))?;
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
    let git_dir = if Path::new(&git_dir_str).is_absolute() {
        PathBuf::from(&git_dir_str)
    } else {
        project_root.join(&git_dir_str)
    };

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
pub fn install_script(directory: &Path, filename: &str, content: &str) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|e| format!("Could not create directory {}: {}", directory.display(), e))?;
    let target = directory.join(filename);
    fs::write(&target, content)
        .map_err(|e| format!("Could not write {}: {}", target.display(), e))?;
    let mut perms = fs::metadata(&target)
        .map_err(|e| format!("Could not read metadata for {}: {}", target.display(), e))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms)
        .map_err(|e| format!("Could not chmod {}: {}", target.display(), e))?;
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

/// Resolve the user's home directory, preferring `$HOME` for testability.
fn home_dir() -> PathBuf {
    match env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => PathBuf::from(env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string())),
    }
}

/// Install a global flow launcher at `~/.local/bin/flow`.
pub fn install_launcher(home: &Path) -> Result<(), String> {
    install_script(&home.join(".local").join("bin"), "flow", LAUNCHER_SCRIPT)
}

/// Warn if `~/.local/bin` is not in PATH.
pub fn check_launcher_path(home: &Path) {
    let local_bin = home.join(".local").join("bin");
    let local_bin_str = local_bin.to_string_lossy().to_string();
    let path_var = env::var("PATH").unwrap_or_default();
    let dirs: Vec<&str> = path_var.split(':').collect();
    if !dirs.contains(&local_bin_str.as_str()) {
        eprintln!(
            "Warning: {} is not in your PATH. \
             Add this to your shell profile:\n  \
             export PATH=\"$HOME/.local/bin:$PATH\"",
            local_bin_str
        );
    }
}

#[derive(ClapArgs)]
pub struct Args {
    /// Project root directory
    pub project_root: String,

    /// Framework name (rails, python, ios, go, rust)
    #[arg(long)]
    pub framework: Option<String>,

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
pub fn run_impl(args: &Args) -> Result<Value, Value> {
    let project_root = PathBuf::from(&args.project_root);
    if !project_root.is_dir() {
        return Err(json!({
            "status": "error",
            "message": format!("Project root not found: {}", args.project_root),
        }));
    }

    let framework = match &args.framework {
        Some(f) if !f.is_empty() => f.clone(),
        _ => {
            return Err(json!({
                "status": "error",
                "message": format!("Missing or invalid --framework argument: {:?}", args.framework),
            }));
        }
    };

    let fw_dir = match frameworks_dir() {
        Some(d) => d,
        None => {
            return Err(json!({
                "status": "error",
                "message": "Frameworks directory not found",
            }));
        }
    };

    if !fw_dir.join(&framework).is_dir() {
        return Err(json!({
            "status": "error",
            "message": format!("Missing or invalid --framework argument: {}", framework),
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

    let config_hash = compute_config_hash(&framework, &fw_dir)
        .map_err(|e| json!({"status": "error", "message": e}))?;
    let setup_hash =
        compute_setup_hash(&p_root).map_err(|e| json!({"status": "error", "message": e}))?;

    merge_settings(&project_root, &framework, &fw_dir)
        .map_err(|e| json!({"status": "error", "message": e}))?;

    write_version_marker(
        &project_root,
        &version,
        &framework,
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

    // Call prime-project in-process
    let prime_project_args = prime_project::Args {
        project_root: args.project_root.clone(),
        framework: framework.clone(),
    };
    let prime_result = match prime_project::run_impl(&prime_project_args) {
        Ok(v) => v
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("error")
            .to_string(),
        Err(v) => v
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("error")
            .to_string(),
    };

    // Call create-dependencies in-process
    let deps_result = create_dependencies::create(&args.project_root, &framework, Some(&fw_dir));
    let deps_status = deps_result
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("error")
        .to_string();

    Ok(json!({
        "status": "ok",
        "settings_merged": true,
        "exclude_updated": exclude_updated,
        "version_marker": true,
        "hook_installed": true,
        "launcher_installed": launcher_installed,
        "framework": framework,
        "prime_project": prime_result,
        "dependencies": deps_status,
    }))
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
        }
        Err(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
            process::exit(1);
        }
    }
}
