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
///
/// Hidden entries (dot-prefixed names) are skipped when the pattern
/// does not itself start with a dot — this matches Python
/// `Path.glob`, whose `*` wildcard does not match leading dots.
/// Without this filter a project containing a stray `.xcodeproj`
/// directory would falsely detect as iOS.
fn matches_glob(dir: &Path, pattern: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix("*.") {
        let suffix = format!(".{}", ext);
        match fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(|e| e.ok()).any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !name.starts_with('.') && name.ends_with(&suffix)
            }),
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

/// Build a name+display_name JSON object from a detect.json config.
fn framework_summary(config: &Value) -> Value {
    json!({
        "name": config.get("name").cloned().unwrap_or(Value::Null),
        "display_name": config.get("display_name").cloned().unwrap_or(Value::Null),
    })
}

/// Return true if the config's `detect_globs` match any file in the project.
fn config_matches_project(config: &Value, project: &Path) -> bool {
    config
        .get("detect_globs")
        .and_then(|v| v.as_array())
        .map(|globs| {
            globs
                .iter()
                .any(|g| g.as_str().is_some_and(|s| matches_glob(project, s)))
        })
        .unwrap_or(false)
}

/// Return the list of detected frameworks for a project root.
pub fn detect(project: &Path, fw_dir: &Path) -> Vec<Value> {
    load_detect_configs(fw_dir)
        .iter()
        .filter(|c| config_matches_project(c, project))
        .map(framework_summary)
        .collect()
}

/// Return the list of all available frameworks from `fw_dir`.
pub fn available_frameworks(fw_dir: &Path) -> Vec<Value> {
    load_detect_configs(fw_dir)
        .iter()
        .map(framework_summary)
        .collect()
}

/// Build the CLI result as a JSON value.
///
/// Returns `Err` on infrastructure failures (missing project root,
/// missing frameworks directory). Error-status responses are returned
/// via `Err` so `run` can exit non-zero; this matches the Python
/// script's behavior of printing JSON + `sys.exit(1)`.
///
/// Loads detect.json configs once and derives both `detected` and
/// `available` from the single load — avoids reading each detect.json
/// twice that the `detect()` + `available_frameworks()` combo would do.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let project_root = PathBuf::from(&args.project_root);
    if !project_root.is_dir() {
        return Err(format!("Project root not found: {}", args.project_root));
    }
    let fw_dir = frameworks_dir().ok_or_else(|| "Plugin root not found".to_string())?;
    let configs = load_detect_configs(&fw_dir);
    let detected: Vec<Value> = configs
        .iter()
        .filter(|c| config_matches_project(c, &project_root))
        .map(framework_summary)
        .collect();
    let available: Vec<Value> = configs.iter().map(framework_summary).collect();
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
