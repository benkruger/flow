//! Data-driven framework auto-detection.
//!
//! Port of lib/detect-framework.py. Reads `frameworks/*/detect.json`
//! to discover which frameworks match a project based on file presence.
//!
//! Usage: `bin/flow detect-framework <project_root>`
//!
//! Output (JSON to stdout):
//!   `{"status": "ok", "detected": [...], "available": [...]}`
//!   `{"status": "error", "message": "..."}`

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use clap::Args as ClapArgs;
use serde_json::{json, Value};

use crate::output::json_error;
use crate::utils::frameworks_dir;

#[derive(ClapArgs)]
pub struct Args {
    /// Project root directory
    pub project_root: String,
}

/// Return true if at least one entry in `dir` matches `pattern`.
///
/// Pattern semantics mirror Python `Path.glob(pattern)` for the small
/// set of patterns actually used in `frameworks/*/detect.json`:
/// literal filenames (e.g. "Gemfile", "go.mod") and `*.ext` wildcards
/// (e.g. "*.xcodeproj"). Both file and directory entries match by name.
fn matches_glob(dir: &Path, pattern: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix("*.") {
        let suffix = format!(".{}", ext);
        match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().ends_with(&suffix)),
            Err(_) => false,
        }
    } else {
        dir.join(pattern).exists()
    }
}

/// Load all detect.json files from `frameworks_dir/*/`, sorted by path.
fn load_detect_configs(fw_dir: &Path) -> Vec<Value> {
    let mut paths: Vec<PathBuf> = match fs::read_dir(fw_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("detect.json"))
            .filter(|p| p.exists())
            .collect(),
        Err(_) => return Vec::new(),
    };
    paths.sort();

    let mut configs = Vec::new();
    for path in paths {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<Value>(&content) {
                configs.push(value);
            }
        }
    }
    configs
}

/// Return the list of detected frameworks for a project root.
pub fn detect(project: &Path, fw_dir: &Path) -> Vec<Value> {
    let configs = load_detect_configs(fw_dir);
    let mut detected = Vec::new();
    for config in configs {
        let globs = match config.get("detect_globs").and_then(|v| v.as_array()) {
            Some(g) => g,
            None => continue,
        };
        let matched = globs
            .iter()
            .any(|g| g.as_str().is_some_and(|s| matches_glob(project, s)));
        if matched {
            detected.push(json!({
                "name": config.get("name").cloned().unwrap_or(Value::Null),
                "display_name": config.get("display_name").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    detected
}

/// Return the list of all available frameworks from `fw_dir`.
pub fn available_frameworks(fw_dir: &Path) -> Vec<Value> {
    load_detect_configs(fw_dir)
        .into_iter()
        .map(|c| {
            json!({
                "name": c.get("name").cloned().unwrap_or(Value::Null),
                "display_name": c.get("display_name").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// Build the CLI result as a JSON value.
///
/// Returns `Err` on infrastructure failures (missing project root,
/// missing frameworks directory). Error-status responses are returned
/// via `Err` so `run` can exit non-zero; this matches the Python
/// script's behavior of printing JSON + `sys.exit(1)`.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let project_root = PathBuf::from(&args.project_root);
    if !project_root.is_dir() {
        return Err(format!("Project root not found: {}", args.project_root));
    }
    let fw_dir = frameworks_dir().ok_or_else(|| "Plugin root not found".to_string())?;
    let detected = detect(&project_root, &fw_dir);
    let available = available_frameworks(&fw_dir);
    Ok(json!({
        "status": "ok",
        "detected": detected,
        "available": available,
    }))
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
        }
        Err(msg) => {
            json_error(&msg, &[]);
            process::exit(1);
        }
    }
}
