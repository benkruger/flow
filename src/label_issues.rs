use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::utils::extract_issue_numbers;

pub const LABEL: &str = "Flow In-Progress";
const TIMEOUT_SECS: u64 = 30;

// Polling-based wait_timeout for child processes. Returns
// `Some(status)` when the child exits before `dur` elapses, `None` on
// timeout. `try_wait` on an owned Child is infallible in practice —
// kernel ECHILD only fires when another party already reaped the
// child, which cannot happen for a Child we still own. Treated as a
// genuine unreachable panic per
// `.claude/rules/testability-means-simplicity.md`.
trait WaitTimeout {
    fn wait_timeout(&mut self, dur: Duration) -> Option<std::process::ExitStatus>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> Option<std::process::ExitStatus> {
        use std::thread;

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match self
                .try_wait()
                .expect("try_wait on owned child is infallible")
            {
                Some(status) => return Some(status),
                None => {
                    if start.elapsed() >= dur {
                        return None;
                    }
                    thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct LabelResult {
    pub labeled: Vec<i64>,
    pub failed: Vec<i64>,
}

/// Add or remove the Flow In-Progress label via an injected
/// child_factory and explicit timeout. Tests inject sh-based factories
/// and small timeouts to drive every spawn outcome (Ok success, Ok
/// non-success, Ok timeout, Err) without spawning real `gh`.
pub fn label_issues_with_runner(
    issue_numbers: &[i64],
    action: &str,
    timeout: Duration,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> LabelResult {
    let mut labeled = Vec::new();
    let mut failed = Vec::new();
    let flag = if action == "add" {
        "--add-label"
    } else {
        "--remove-label"
    };

    for &num in issue_numbers {
        let num_str = num.to_string();
        let args = ["issue", "edit", num_str.as_str(), flag, LABEL];
        let result = child_factory(&args);

        match result {
            Ok(mut child) => match child.wait_timeout(timeout) {
                Some(status) if status.success() => labeled.push(num),
                Some(_) => failed.push(num),
                // Timeout — kill the child and record as failed.
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    failed.push(num);
                }
            },
            Err(_) => failed.push(num),
        }
    }

    LabelResult { labeled, failed }
}

/// Default timeout for label operations — `start_init` and the CLI
/// dispatcher pass this to `label_issues_with_runner` to bound the gh
/// spawn wait.
pub fn default_timeout() -> Duration {
    Duration::from_secs(TIMEOUT_SECS)
}

/// Production gh child factory used by the CLI dispatcher and by
/// `start_init`. Exposed as a function (rather than an inline closure)
/// so a single test can exercise the spawn path.
pub fn gh_child_factory(args: &[&str]) -> std::io::Result<Child> {
    Command::new("gh")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[derive(Parser, Debug)]
#[command(
    name = "label-issues",
    about = "Add or remove Flow In-Progress label on issues"
)]
#[command(group(clap::ArgGroup::new("action").args(["add", "remove"]).required(true)))]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Add label
    #[arg(long)]
    pub add: bool,

    /// Remove label
    #[arg(long)]
    pub remove: bool,
}

/// Main-arm dispatcher with injected child_factory. Reads the state
/// file, extracts issue numbers from the prompt, and labels them via
/// the injected factory. Tests pass a mock child_factory to exercise
/// the gh-spawning branch without spawning real `gh`. Returns
/// `(value, exit_code)`: `(error+message+step, 1)` on state-file read
/// or parse failure, `(ok+labeled+failed, 0)` on success.
pub fn run_impl_main_with_runner(
    args: Args,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> (Value, i32) {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        return (
            json!({
                "status": "error",
                "step": "read_state",
                "message": format!("State file not found: {}", args.state_file),
            }),
            1,
        );
    }

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "step": "read_state",
                    "message": format!("Failed to read state file: {}", e),
                }),
                1,
            );
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "step": "parse_state",
                    "message": format!("Failed to parse state file: {}", e),
                }),
                1,
            );
        }
    };

    let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let issue_numbers = extract_issue_numbers(prompt);
    let action = if args.add { "add" } else { "remove" };
    let result = label_issues_with_runner(
        &issue_numbers,
        action,
        Duration::from_secs(TIMEOUT_SECS),
        child_factory,
    );

    (
        json!({
            "status": "ok",
            "labeled": result.labeled,
            "failed": result.failed,
        }),
        0,
    )
}

/// Production main-arm dispatcher: wires `run_impl_main_with_runner`
/// to the real `gh` subprocess via `gh_child_factory`.
pub fn run_impl_main(args: Args) -> (Value, i32) {
    run_impl_main_with_runner(args, &gh_child_factory)
}
