//! Port of lib/complete-merge.py — consolidated Complete phase merge.
//!
//! Absorbs Step 8: freshness check + squash merge.
//!
//! Usage: bin/flow complete-merge --pr <number> --state-file <path>
//!
//! Output (JSON to stdout):
//!   Merged:     {"status": "merged", "pr_number": N}
//!   CI rerun:   {"status": "ci_rerun", "pushed": true, "pr_number": N}
//!   Conflict:   {"status": "conflict", "conflict_files": [...], "pr_number": N}
//!   CI pending: {"status": "ci_pending", "pr_number": N}
//!   Max retry:  {"status": "max_retries", "pr_number": N}
//!   Error:      {"status": "error", "message": "...", "pr_number": N}

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::lock::mutate_state;

const NETWORK_TIMEOUT: u64 = 60;
const MERGE_STEP: i64 = 5;

type CmdResult = Result<(i32, String, String), String>;

#[derive(Parser, Debug)]
#[command(name = "complete-merge", about = "FLOW Complete phase merge")]
pub struct Args {
    /// PR number to merge
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
}

/// Locate bin/flow via current_exe traversal, falling back to "bin/flow".
fn bin_flow_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent()?.parent()?.parent().map(|d| d.to_path_buf()))
        .map(|d: PathBuf| d.join("bin").join("flow"))
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "bin/flow".to_string())
}

/// Run a subprocess command with a timeout. `args[0]` is the program.
fn run_cmd_with_timeout(args: &[&str], timeout_secs: u64) -> CmdResult {
    let (program, rest) = match args.split_first() {
        Some(p) => p,
        None => return Err("empty command".to_string()),
    };
    let mut child = Command::new(program)
        .args(rest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;

    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let code = output.status.code().unwrap_or(1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok((code, stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Timed out after {}s", timeout_secs));
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                std::thread::sleep(poll_interval.min(remaining));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Build an error result with pr_number.
fn error_result(message: &str, pr_number: i64) -> Value {
    json!({
        "status": "error",
        "message": message,
        "pr_number": pr_number,
    })
}

/// Core complete-merge logic with injectable runner.
pub fn complete_merge_inner(
    pr_number: i64,
    state_file: &str,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
    // Set step counter if state file exists
    let state_path = Path::new(state_file);
    if state_path.exists() {
        let _ = mutate_state(state_path, |s| {
            if !(s.is_object() || s.is_null()) {
                return;
            }
            s["complete_step"] = json!(MERGE_STEP);
        });
    }

    // Run check-freshness
    let freshness_result = runner(
        &[bin_flow, "check-freshness", "--state-file", state_file],
        NETWORK_TIMEOUT,
    );

    let (_code, stdout, _stderr) = match freshness_result {
        Err(e) => {
            return error_result(&e, pr_number);
        }
        Ok(triple) => triple,
    };

    let freshness: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return error_result(
                &format!("Invalid JSON from check-freshness: {}", stdout),
                pr_number,
            );
        }
    };

    let freshness_status = freshness
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match freshness_status {
        "max_retries" => json!({"status": "max_retries", "pr_number": pr_number}),
        "error" => {
            let msg = freshness
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("check-freshness failed");
            error_result(msg, pr_number)
        }
        "conflict" => {
            let files = freshness.get("files").cloned().unwrap_or(json!([]));
            json!({
                "status": "conflict",
                "conflict_files": files,
                "pr_number": pr_number,
            })
        }
        "merged" => {
            // Main had new commits, merged into branch — push
            match runner(&["git", "push"], NETWORK_TIMEOUT) {
                Err(e) => error_result(
                    &format!("Push failed after freshness merge: {}", e),
                    pr_number,
                ),
                Ok((code, _, stderr)) => {
                    if code != 0 {
                        error_result(
                            &format!("Push failed after freshness merge: {}", stderr.trim()),
                            pr_number,
                        )
                    } else {
                        json!({
                            "status": "ci_rerun",
                            "pushed": true,
                            "pr_number": pr_number,
                        })
                    }
                }
            }
        }
        "up_to_date" => {
            // Proceed to squash merge
            let pr_str = pr_number.to_string();
            match runner(
                &["gh", "pr", "merge", &pr_str, "--squash"],
                NETWORK_TIMEOUT,
            ) {
                Err(e) => error_result(&e, pr_number),
                Ok((code, _, stderr)) => {
                    if code == 0 {
                        json!({"status": "merged", "pr_number": pr_number})
                    } else {
                        let stderr_trim = stderr.trim();
                        if stderr_trim.contains("base branch policy") {
                            json!({"status": "ci_pending", "pr_number": pr_number})
                        } else {
                            error_result(stderr_trim, pr_number)
                        }
                    }
                }
            }
        }
        other => error_result(
            &format!("Unexpected check-freshness status: {}", other),
            pr_number,
        ),
    }
}

/// Production wrapper.
pub fn complete_merge(pr_number: i64, state_file: &str) -> Value {
    complete_merge_inner(
        pr_number,
        state_file,
        &bin_flow_path(),
        &run_cmd_with_timeout,
    )
}

/// CLI entry point. Exits 1 if status != "merged" (matches Python behavior).
pub fn run(args: Args) {
    let result = complete_merge(args.pr, &args.state_file);
    println!("{}", result);
    if result["status"] != "merged" {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;

    fn mock_runner(responses: Vec<CmdResult>) -> impl Fn(&[&str], u64) -> CmdResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |_args: &[&str], _timeout: u64| -> CmdResult {
            queue
                .borrow_mut()
                .pop_front()
                .expect("mock_runner: no more responses")
        }
    }

    fn ok(stdout: &str) -> CmdResult {
        Ok((0, stdout.to_string(), String::new()))
    }

    fn ok_empty() -> CmdResult {
        Ok((0, String::new(), String::new()))
    }

    fn fail_with_stdout_stderr(stdout: &str, stderr: &str) -> CmdResult {
        Ok((1, stdout.to_string(), stderr.to_string()))
    }

    fn err(msg: &str) -> CmdResult {
        Err(msg.to_string())
    }

    fn write_state(path: &Path) {
        let state = json!({
            "schema_version": 1,
            "branch": "test-feature",
            "pr_number": 42,
            "complete_step": 4,
            "phases": {}
        });
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }

    // --- happy paths ---

    #[test]
    fn up_to_date_and_merge_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            ok("merged"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "merged");
        assert_eq!(result["pr_number"], 42);
    }

    #[test]
    fn main_moved_ci_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "merged"}"#),
            ok_empty(), // git push
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ci_rerun");
        assert_eq!(result["pushed"], true);
        assert_eq!(result["pr_number"], 42);
    }

    #[test]
    fn merge_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![fail_with_stdout_stderr(
            r#"{"status": "conflict", "files": ["lib/foo.py", "lib/bar.py"]}"#,
            "",
        )]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "conflict");
        let files: Vec<String> = result["conflict_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(files, vec!["lib/foo.py", "lib/bar.py"]);
        assert_eq!(result["pr_number"], 42);
    }

    #[test]
    fn max_retries() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![fail_with_stdout_stderr(
            r#"{"status": "max_retries", "retries": 3}"#,
            "",
        )]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "max_retries");
        assert_eq!(result["pr_number"], 42);
    }

    #[test]
    fn branch_protection_ci_pending() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            fail_with_stdout_stderr("", "base branch policy prohibits the merge"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ci_pending");
        assert_eq!(result["pr_number"], 42);
    }

    #[test]
    fn merge_fails_other_reason() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            fail_with_stdout_stderr("", "unknown merge error"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("unknown merge error"));
    }

    #[test]
    fn check_freshness_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![fail_with_stdout_stderr(
            r#"{"status": "error", "step": "fetch", "message": "network error"}"#,
            "",
        )]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("network error"));
    }

    #[test]
    fn step_counter_set() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            ok("merged"),
        ]);

        complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        let state_content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&state_content).unwrap();
        assert_eq!(state["complete_step"], json!(5));
    }

    #[test]
    fn push_failure_after_freshness_merge() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "merged"}"#),
            fail_with_stdout_stderr("", "remote rejected"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("push"));
    }

    #[test]
    fn check_freshness_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![fail_with_stdout_stderr("not json at all", "")]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn timeout_handling() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![err("Timed out after 60s")]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn unknown_freshness_status() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![ok(r#"{"status": "unexpected_value"}"#)]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("unexpected"));
    }

    #[test]
    fn missing_state_file_skips_step_counter() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("nonexistent.json");

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            ok("merged"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        // Still succeeds — step counter is best-effort
        assert_eq!(result["status"], "merged");
    }

    #[test]
    fn object_guard_non_object_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        // Write an array instead of an object — mutate_state closure must not panic
        fs::write(&state_path, "[1, 2, 3]").unwrap();

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            ok("merged"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        // Should not panic; returns merged
        assert_eq!(result["status"], "merged");
    }

    #[test]
    fn conflict_with_missing_files_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        // conflict payload without "files" key
        let runner = mock_runner(vec![fail_with_stdout_stderr(
            r#"{"status": "conflict"}"#,
            "",
        )]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "conflict");
        assert_eq!(result["conflict_files"], json!([]));
    }

    #[test]
    fn check_freshness_runner_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![err("spawn failed: no such binary")]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("spawn failed"));
    }

    #[test]
    fn squash_merge_transport_error_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            err("Timed out after 60s"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn push_transport_error_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        write_state(&state_path);

        let runner = mock_runner(vec![
            ok(r#"{"status": "merged"}"#),
            err("Timed out after 60s"),
        ]);

        let result = complete_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }
}
