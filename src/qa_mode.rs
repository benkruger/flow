//! Manage dev-mode plugin_root redirection in .flow.json.
//!
//! Usage:
//!   bin/flow qa-mode --start --local-path <path>
//!   bin/flow qa-mode --start --local-path <path> --flow-json <path>
//!   bin/flow qa-mode --stop
//!   bin/flow qa-mode --stop --flow-json <path>
//!
//! Start: saves current plugin_root as plugin_root_backup, overwrites
//! plugin_root with the local FLOW source path.
//!
//! Stop: restores plugin_root from plugin_root_backup, removes the
//! backup key.

use std::path::Path;

use clap::{ArgGroup, Parser};
use serde_json::{json, Value};

use crate::git::project_root;

#[derive(Parser, Debug)]
#[command(name = "qa-mode", about = "Manage dev-mode plugin_root redirection")]
#[command(group(ArgGroup::new("action").required(true).args(["start", "stop"])))]
pub struct Args {
    /// Switch to dev mode
    #[arg(long)]
    pub start: bool,

    /// Switch back to marketplace mode
    #[arg(long)]
    pub stop: bool,

    /// Path to local FLOW source (required with --start)
    #[arg(long)]
    pub local_path: Option<String>,

    /// Path to .flow.json (default: <project_root>/.flow.json)
    #[arg(long)]
    pub flow_json: Option<String>,
}

/// Redirect plugin_root to local source for dev testing.
///
/// Reads .flow.json, validates preconditions, saves plugin_root as
/// plugin_root_backup, overwrites plugin_root with local_source_path.
///
/// Returns JSON value with status, plugin_root, and backup on success,
/// or status and message on error.
fn start_impl(flow_json_path: &Path, local_source_path: &Path) -> Value {
    if !flow_json_path.exists() {
        return json!({
            "status": "error",
            "message": format!(".flow.json not found at {}", flow_json_path.display())
        });
    }

    let content = match std::fs::read_to_string(flow_json_path) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to read .flow.json: {}", e)
            });
        }
    };

    let mut data: Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to parse .flow.json: {}", e)
            });
        }
    };

    if data.get("plugin_root").and_then(|v| v.as_str()).is_none() {
        return json!({
            "status": "error",
            "message": "plugin_root not found in .flow.json — run /flow:flow-prime first"
        });
    }

    if data.get("plugin_root_backup").is_some() {
        return json!({
            "status": "error",
            "message": "Already in dev mode — plugin_root_backup exists. Run --stop first."
        });
    }

    if !local_source_path.exists() {
        return json!({
            "status": "error",
            "message": format!("Local source path does not exist: {}", local_source_path.display())
        });
    }

    if !local_source_path.join("bin").join("flow").exists() {
        return json!({
            "status": "error",
            "message": format!("No bin/flow found in {} — not a FLOW source directory", local_source_path.display())
        });
    }

    let backup = data["plugin_root"].as_str().unwrap().to_string();
    let local_str = local_source_path.to_string_lossy().to_string();
    data["plugin_root_backup"] = json!(backup);
    data["plugin_root"] = json!(local_str);

    let output = serde_json::to_string(&data).unwrap() + "\n";
    if let Err(e) = std::fs::write(flow_json_path, output) {
        return json!({
            "status": "error",
            "message": format!("Failed to write .flow.json: {}", e)
        });
    }

    json!({
        "status": "ok",
        "plugin_root": local_str,
        "backup": backup
    })
}

/// Restore plugin_root from backup after dev testing.
///
/// Reads .flow.json, restores plugin_root from plugin_root_backup,
/// removes the backup key.
///
/// Returns JSON value with status and restored path on success,
/// or status and message on error.
fn stop_impl(flow_json_path: &Path) -> Value {
    if !flow_json_path.exists() {
        return json!({
            "status": "error",
            "message": format!(".flow.json not found at {}", flow_json_path.display())
        });
    }

    let content = match std::fs::read_to_string(flow_json_path) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to read .flow.json: {}", e)
            });
        }
    };

    let mut data: Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to parse .flow.json: {}", e)
            });
        }
    };

    let restored = match data.get("plugin_root_backup").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return json!({
                "status": "error",
                "message": "Not in dev mode — no plugin_root_backup found in .flow.json"
            });
        }
    };

    data["plugin_root"] = json!(restored);
    // `data` must be an Object here: the earlier
    // `data.get("plugin_root_backup")` check only returns `Some(s)` for
    // an object that contains the key, so non-object shapes have
    // already been short-circuited above.
    data.as_object_mut()
        .expect("data is a JSON object — plugin_root_backup lookup would have failed otherwise")
        .remove("plugin_root_backup");

    let output = serde_json::to_string(&data).unwrap() + "\n";
    if let Err(e) = std::fs::write(flow_json_path, output) {
        return json!({
            "status": "error",
            "message": format!("Failed to write .flow.json: {}", e)
        });
    }

    json!({
        "status": "ok",
        "restored": restored
    })
}

/// CLI entry point — resolves flow_json default, dispatches to start/stop.
///
/// Returns Ok(Value) for both success and status-error responses.
/// Returns Err(String) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let flow_json_path = if let Some(ref path) = args.flow_json {
        std::path::PathBuf::from(path)
    } else {
        let root = project_root();
        root.join(".flow.json")
    };

    if args.start {
        let local_path = match &args.local_path {
            Some(p) => p,
            None => {
                return Ok(json!({
                    "status": "error",
                    "message": "--local-path is required with --start"
                }));
            }
        };
        Ok(start_impl(&flow_json_path, Path::new(local_path)))
    } else {
        Ok(stop_impl(&flow_json_path))
    }
}
