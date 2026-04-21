//! Close GitHub issues referenced in the FLOW start prompt.
//!
//! Reads the state file, extracts #N patterns from the prompt field,
//! and closes each issue via gh CLI after the PR is merged.
//!
//! Usage: bin/flow close-issues --state-file <path>
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "closed": [{"number": 83, "url": "..."}], "failed": [{"number": 89, "error": "not found"}]}
//!
//! Tests live at tests/close_issues.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::utils::extract_issue_numbers;

#[derive(Parser, Debug)]
#[command(name = "close-issues", about = "Close issues from FLOW prompt")]
pub struct Args {
    /// Path to state JSON file
    #[arg(long = "state-file")]
    pub state_file: String,
}

/// Close each issue via the injected child_factory. Returns
/// (closed, failed) lists. Tests inject sh-based child factories to
/// drive the success/failure outcomes for each issue without spawning
/// real `gh`. Production wraps this with a closure that calls
/// `Command::new("gh")`.
pub fn close_issues_with_runner(
    issue_numbers: &[i64],
    repo: Option<&str>,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> (Vec<Value>, Vec<Value>) {
    close_issues_with_runner_and_timeout(issue_numbers, repo, LOCAL_TIMEOUT, child_factory)
}

/// Seam-injected variant accepting a custom timeout (in seconds). Tests
/// pass `0` so the elapsed-time check trips on the first poll and the
/// timeout-arm message is exercised in `close_single_issue`.
pub fn close_issues_with_runner_and_timeout(
    issue_numbers: &[i64],
    repo: Option<&str>,
    timeout_secs: u64,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> (Vec<Value>, Vec<Value>) {
    let mut closed = Vec::new();
    let mut failed = Vec::new();
    let timeout = Duration::from_secs(timeout_secs);

    for &num in issue_numbers {
        match close_single_issue(num, repo, timeout, child_factory) {
            Ok(()) => {
                let mut entry = serde_json::Map::new();
                entry.insert("number".to_string(), json!(num));
                if let Some(r) = repo {
                    entry.insert(
                        "url".to_string(),
                        json!(format!("https://github.com/{}/issues/{}", r, num)),
                    );
                }
                closed.push(Value::Object(entry));
            }
            Err(e) => {
                failed.push(json!({"number": num, "error": e}));
            }
        }
    }

    (closed, failed)
}

pub fn close_single_issue(
    number: i64,
    repo: Option<&str>,
    timeout: Duration,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> Result<(), String> {
    let mut cmd_args = vec!["issue", "close"];
    let num_str = number.to_string();
    cmd_args.push(&num_str);
    if let Some(r) = repo {
        cmd_args.push("--repo");
        cmd_args.push(r);
    }

    let mut child = match child_factory(&cmd_args) {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn: {}", e)),
    };

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            let (success, stderr_bytes) = child
                .wait_with_output()
                .map(|o| (o.status.success(), o.stderr))
                .unwrap_or((false, Vec::new()));
            if success {
                return Ok(());
            }
            return Err(String::from_utf8_lossy(&stderr_bytes).trim().to_string());
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeout".to_string());
        }
        std::thread::sleep(poll_interval.min(timeout - start.elapsed()));
    }
}

/// Main-arm dispatcher with injected child_factory. The production
/// wrapper `run_impl_main` calls this with `&gh_child_factory`;
/// integration tests drive this through the subprocess by putting a
/// stub `gh` on PATH.
fn run_impl_main_with_runner(
    args: Args,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> (Value, i32) {
    let content = match fs::read_to_string(&args.state_file) {
        Ok(c) => c,
        Err(e) => {
            return (
                json!({"status": "error", "message": format!("Could not read state file: {}", e)}),
                1,
            );
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                json!({"status": "error", "message": format!("Could not read state file: {}", e)}),
                1,
            );
        }
    };

    let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let repo = state.get("repo").and_then(|v| v.as_str());
    let issue_numbers = extract_issue_numbers(prompt);

    let (closed, failed) = close_issues_with_runner(&issue_numbers, repo, child_factory);

    (
        json!({
            "status": "ok",
            "closed": closed,
            "failed": failed,
        }),
        0,
    )
}

/// Production main-arm dispatcher: wires `run_impl_main_with_runner`
/// to the real `gh` subprocess.
pub fn run_impl_main(args: Args) -> (Value, i32) {
    run_impl_main_with_runner(args, &|cmd_args| {
        Command::new("gh")
            .args(cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
}
