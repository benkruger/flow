//! Promote permissions from settings.local.json into settings.json.
//!
//! Reads
//! `.claude/settings.local.json`, merges new `permissions.allow` entries
//! into `.claude/settings.json`, deletes settings.local.json, and
//! outputs JSON.
//!
//! Usage: `bin/flow promote-permissions --worktree-path <path>`
//!
//! Output (JSON to stdout):
//!   `{"status": "skipped", "reason": "no_local_file"}`
//!   `{"status": "ok", "promoted": [...], "already_present": N}`
//!   `{"status": "error", "message": "..."}`

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use clap::Args as ClapArgs;
use serde_json::{json, Value};

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the worktree or project root
    #[arg(long = "worktree-path")]
    pub worktree_path: String,
}

/// Merge settings.local.json allow entries into settings.json.
///
/// Returns one of three result shapes: `skipped` (no local file present),
/// `ok` (merged successfully with the list of newly promoted entries),
/// or `error` (parse, write, or shape failure with a displayable message).
/// The local file is deleted on success; deletion failures are swallowed
/// because the next promote() call will retry.
pub fn promote(worktree_path: &Path) -> Value {
    let local_path = worktree_path.join(".claude").join("settings.local.json");
    let settings_path = worktree_path.join(".claude").join("settings.json");

    if !local_path.exists() {
        return json!({"status": "skipped", "reason": "no_local_file"});
    }

    let local_data: Value = match read_json(&local_path) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not parse settings.local.json: {}", e),
            })
        }
    };

    let local_allow: Vec<String> = local_data
        .get("permissions")
        .and_then(|v| v.get("allow"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !settings_path.exists() {
        return json!({
            "status": "error",
            "message": "settings.json does not exist",
        });
    }

    let mut settings_data: Value = match read_json(&settings_path) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not parse settings.json: {}", e),
            })
        }
    };

    let mut existing_allow: Vec<Value> = settings_data
        .get("permissions")
        .and_then(|v| v.get("allow"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut existing_set: HashSet<String> = existing_allow
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mut promoted: Vec<String> = Vec::new();
    let mut already_present: i64 = 0;
    for entry in local_allow {
        if existing_set.contains(&entry) {
            already_present += 1;
        } else {
            promoted.push(entry.clone());
            existing_allow.push(Value::String(entry.clone()));
            existing_set.insert(entry);
        }
    }

    if !(settings_data.is_object() || settings_data.is_null()) {
        return json!({
            "status": "error",
            "message": "settings.json is not a JSON object",
        });
    }

    // Guard both the top-level settings object and the nested `permissions`
    // value — if either is not an object, assigning `["permissions"]["allow"]`
    // would trigger a serde_json IndexMut panic. Replace a malformed
    // permissions value with a fresh empty object so the merge can proceed.
    if !matches!(settings_data.get("permissions"), Some(v) if v.is_object()) {
        settings_data["permissions"] = json!({});
    }
    settings_data["permissions"]["allow"] = Value::Array(existing_allow);

    let serialized = match serde_json::to_string_pretty(&settings_data) {
        Ok(s) => s,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not write settings.json: {}", e),
            })
        }
    };

    let mut bytes = serialized.into_bytes();
    bytes.push(b'\n');
    if let Err(e) = fs::write(&settings_path, &bytes) {
        return json!({
            "status": "error",
            "message": format!("Could not write settings.json: {}", e),
        });
    }

    // Best-effort cleanup: tolerate I/O errors here because the next
    // promote() call retries the merge and the deletion.
    let _ = fs::remove_file(&local_path);

    json!({
        "status": "ok",
        "promoted": promoted,
        "already_present": already_present,
    })
}

/// Read a JSON file and parse it. Bundles `io::Error` and
/// `serde_json::Error` into a single displayable error string so the
/// caller can emit a unified `"Could not parse <path>: <reason>"`
/// message without inspecting which layer failed.
fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// Build the CLI result as a JSON value.
///
/// Returns `Err` when the result `status` is `"error"` so `run` can
/// exit non-zero with JSON output, while keeping `ok`/`skipped` on
/// the `Ok` path.
pub fn run_impl(args: &Args) -> Result<Value, Value> {
    let worktree = PathBuf::from(&args.worktree_path);
    let result = promote(&worktree);
    if result.get("status").and_then(|v| v.as_str()) == Some("error") {
        Err(result)
    } else {
        Ok(result)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a fresh temporary directory for use as a worktree root.
    fn setup_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Write `content` as `.claude/settings.local.json` inside `dir`,
    /// creating the `.claude/` directory if needed.
    fn write_local(dir: &Path, content: &str) {
        let claude_dir = dir.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("settings.local.json"), content).unwrap();
    }

    /// Write `content` as `.claude/settings.json` inside `dir`,
    /// creating the `.claude/` directory if needed.
    fn write_settings(dir: &Path, content: &str) {
        let claude_dir = dir.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("settings.json"), content).unwrap();
    }

    // --- promote ---

    #[test]
    fn promote_non_object_settings_returns_error() {
        // settings.json containing a JSON array is rejected before
        // the IndexMut assignment that would otherwise panic.
        let dir = setup_dir();
        write_local(
            dir.path(),
            r#"{"permissions": {"allow": ["Bash(echo *)"]}}"#,
        );
        write_settings(dir.path(), "[1, 2, 3]");
        let result = promote(dir.path());
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not a JSON object"));
    }

    // --- run_impl ---

    #[test]
    fn run_impl_skipped_is_ok() {
        let dir = setup_dir();
        let args = Args {
            worktree_path: dir.path().to_string_lossy().to_string(),
        };
        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "skipped");
    }

    #[test]
    fn run_impl_error_is_err() {
        let dir = setup_dir();
        write_local(
            dir.path(),
            r#"{"permissions": {"allow": ["Bash(echo *)"]}}"#,
        );
        // No settings.json → error
        let args = Args {
            worktree_path: dir.path().to_string_lossy().to_string(),
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err()["status"], "error");
    }
}
