//! Consolidated start-init: lock acquire + prime-check + upgrade-check +
//! prompt write + init-state + label-issues in a single command.
//!
//! Reduces the first ~8 tool calls of flow-start to 1. Returns JSON with
//! status "ready" (proceed to start-gate), "locked" (another start holds
//! the lock), or "error" (stop and report).
//!
//! Return type is `Result<Value, String>`: status-error JSON goes through
//! `Ok` with a `status: error` field. `Err(String)` is reserved for
//! infrastructure failures (plugin root not found, etc.) that should exit 1.

use std::fs;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::{acquire, queue_path, release};
use crate::commands::start_step::update_step;
use crate::git::project_root;
use crate::label_issues::label_issues;
use crate::output::json_error;
use crate::prime_check;
use crate::upgrade_check::{self, GhResult};
use crate::utils::{extract_issue_numbers, plugin_root};

#[derive(Parser, Debug)]
#[command(name = "start-init", about = "Consolidated start initialization")]
pub struct Args {
    /// Feature name (sanitized form for lock queue entry)
    pub feature_name: String,

    /// Override all skills to fully autonomous preset
    #[arg(long)]
    pub auto: bool,

    /// Path to file containing start prompt
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,
}

/// Testable entry point.
///
/// Returns `Ok(json)` for all paths (ready, locked, error).
/// Returns `Err(String)` only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let queue_dir = queue_path(&root);
    let state_dir = root.join(".flow-states");
    let _ = fs::create_dir_all(&state_dir);

    let plug_root = plugin_root()
        .ok_or_else(|| "CLAUDE_PLUGIN_ROOT not set and could not detect plugin root".to_string())?;

    // Step 1: Acquire lock
    let lock_result = acquire(&args.feature_name, &queue_dir);
    let _ = append_log(
        &root,
        &args.feature_name,
        &format!("[Phase 1] start-init — lock acquire ({})", lock_result["status"]),
    );

    if lock_result["status"] == "locked" {
        return Ok(json!({
            "status": "locked",
            "feature": lock_result["feature"],
            "lock_path": lock_result["lock_path"],
        }));
    }

    // Helper: release lock on error and return error JSON
    let release_and_error = |msg: &str, step: &str| -> Value {
        release(&args.feature_name, &queue_dir);
        json!({
            "status": "error",
            "message": msg,
            "step": step,
        })
    };

    // Step 2: Prime check
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let prime_result = match prime_check::run_impl(&cwd, &plug_root) {
        Ok(v) => v,
        Err(e) => {
            let _ = append_log(&root, &args.feature_name, &format!("[Phase 1] start-init — prime-check infrastructure error: {}", e));
            return Ok(release_and_error(&e, "prime_check"));
        }
    };

    let _ = append_log(
        &root,
        &args.feature_name,
        &format!("[Phase 1] start-init — prime-check ({})", prime_result["status"]),
    );

    if prime_result["status"] == "error" {
        let msg = prime_result["message"].as_str().unwrap_or("Prime check failed").to_string();
        return Ok(release_and_error(&msg, "prime_check"));
    }

    // Capture version info for response
    let auto_upgraded = prime_result.get("auto_upgraded").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut version_info = json!({});
    if auto_upgraded {
        version_info["auto_upgraded"] = json!(true);
        if let Some(old) = prime_result.get("old_version") {
            version_info["old_version"] = old.clone();
        }
        if let Some(new) = prime_result.get("new_version") {
            version_info["new_version"] = new.clone();
        }
    }

    // Step 3: Upgrade check (best-effort, never errors)
    let plugin_json = plug_root.join(".claude-plugin").join("plugin.json");
    let mut gh_cmd = |owner_repo: &str, timeout_secs: u64| -> GhResult {
        upgrade_check::run_gh_cmd(owner_repo, timeout_secs)
    };
    let upgrade_result = upgrade_check::upgrade_check_impl(&plugin_json, 10, &mut gh_cmd);
    let _ = append_log(
        &root,
        &args.feature_name,
        &format!("[Phase 1] start-init — upgrade-check ({})", upgrade_result["status"]),
    );

    // Step 4: Call init-state as subprocess
    let self_exe = std::env::current_exe()
        .map_err(|e| format!("Could not determine current executable: {}", e))?;
    let mut cmd_args = vec![
        "init-state".to_string(),
        args.feature_name.clone(),
        "--start-step".to_string(),
        "1".to_string(),
        "--start-steps-total".to_string(),
        "5".to_string(),
    ];
    if let Some(ref pf) = args.prompt_file {
        cmd_args.push("--prompt-file".to_string());
        cmd_args.push(pf.clone());
    }
    if args.auto {
        cmd_args.push("--auto".to_string());
    }

    let init_output = std::process::Command::new(&self_exe)
        .args(&cmd_args)
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("Failed to spawn init-state: {}", e))?;

    let init_stdout = String::from_utf8_lossy(&init_output.stdout);
    let init_json: Value = init_stdout
        .trim()
        .lines()
        .last()
        .and_then(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(|| json!({"status": "error", "message": "Could not parse init-state output"}));

    let _ = append_log(
        &root,
        &args.feature_name,
        &format!("[Phase 1] start-init — init-state ({})", init_json["status"]),
    );

    if init_json["status"] == "error" {
        let msg = init_json["message"].as_str().unwrap_or("init-state failed").to_string();
        let step = init_json["step"].as_str().unwrap_or("init_state").to_string();
        return Ok(release_and_error(&msg, &step));
    }

    let branch = init_json["branch"]
        .as_str()
        .unwrap_or(&args.feature_name)
        .to_string();

    // Update step counter for TUI (step 1 = init)
    let state_path = state_dir.join(format!("{}.json", branch));
    update_step(&state_path, 1);

    // Step 5: Label issues (best-effort)
    // Read the prompt from the state file to extract issue numbers
    let prompt = init_json["prompt"]
        .as_str()
        .or_else(|| {
            // Fall back to reading the state file for the prompt
            fs::read_to_string(&state_path)
                .ok()
                .and_then(|content| {
                    serde_json::from_str::<Value>(&content)
                        .ok()
                        .and_then(|state| state["prompt"].as_str().map(String::from))
                })
                .as_deref()
                .map(|_| "") // This branch doesn't work well with lifetimes
        })
        .unwrap_or("");

    let issue_numbers = extract_issue_numbers(prompt);
    let mut labels_result = json!({});
    if !issue_numbers.is_empty() {
        let result = label_issues(&issue_numbers, "add");
        labels_result = json!({
            "labeled": result.labeled,
            "failed": result.failed,
        });
        let _ = append_log(
            &root,
            &branch,
            &format!(
                "[Phase 1] start-init — label-issues (labeled: {:?}, failed: {:?})",
                result.labeled, result.failed
            ),
        );
    }

    // Build response
    let mut response = json!({
        "status": "ready",
        "branch": branch,
        "state_file": format!(".flow-states/{}.json", branch),
    });

    if auto_upgraded {
        response["auto_upgraded"] = json!(true);
        if let Some(old) = version_info.get("old_version") {
            response["old_version"] = old.clone();
        }
        if let Some(new) = version_info.get("new_version") {
            response["new_version"] = new.clone();
        }
    }

    if upgrade_result["status"] != "current" && upgrade_result["status"] != "unknown" {
        response["upgrade"] = upgrade_result;
    }

    if !issue_numbers.is_empty() {
        response["labels"] = labels_result;
    }

    Ok(response)
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}
