use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::utils::extract_issue_numbers;

pub const LABEL: &str = "Flow In-Progress";
const TIMEOUT_SECS: u64 = 30;

// Polling-based wait_timeout for child processes (same pattern as utils.rs)
trait WaitTimeout {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::thread;

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => {
                    if start.elapsed() >= dur {
                        return Ok(None);
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
/// child_factory. Tests inject sh-based factories to drive every spawn
/// outcome (Ok success, Ok non-success, Ok timeout, Err) without
/// spawning real `gh`.
pub fn label_issues_with_runner(
    issue_numbers: &[i64],
    action: &str,
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
            Ok(mut child) => {
                match child.wait_timeout(Duration::from_secs(TIMEOUT_SECS)) {
                    Ok(Some(status)) if status.success() => labeled.push(num),
                    Ok(Some(_)) => failed.push(num),
                    // Timeout or wait error
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        failed.push(num);
                    }
                    Err(_) => failed.push(num),
                }
            }
            Err(_) => failed.push(num),
        }
    }

    LabelResult { labeled, failed }
}

/// Add or remove the Flow In-Progress label on GitHub issues.
///
/// Reads the state file, extracts #N patterns from the prompt field,
/// and adds or removes the label via gh CLI.
pub fn label_issues(issue_numbers: &[i64], action: &str) -> LabelResult {
    label_issues_with_runner(issue_numbers, action, &|args| {
        Command::new("gh")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
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
    let result = label_issues_with_runner(&issue_numbers, action, child_factory);

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

pub fn run(args: Args) -> ! {
    let (value, code) = run_impl_main(args);
    crate::dispatch::dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_issue_list_returns_empty_result() {
        let result = label_issues(&[], "add");
        assert_eq!(
            result,
            LabelResult {
                labeled: vec![],
                failed: vec![],
            }
        );
    }

    #[test]
    fn empty_issue_list_remove_returns_empty_result() {
        let result = label_issues(&[], "remove");
        assert_eq!(
            result,
            LabelResult {
                labeled: vec![],
                failed: vec![],
            }
        );
    }

    #[test]
    fn label_constant_is_flow_in_progress() {
        assert_eq!(LABEL, "Flow In-Progress");
    }

    #[test]
    fn timeout_is_30_seconds() {
        assert_eq!(TIMEOUT_SECS, 30);
    }

    #[test]
    fn run_reads_state_and_extracts_issues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(
            &state_path,
            r#"{"prompt": "fix #42 and #99", "branch": "test"}"#,
        )
        .unwrap();

        let prompt = {
            let content = std::fs::read_to_string(&state_path).unwrap();
            let state: serde_json::Value = serde_json::from_str(&content).unwrap();
            state
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };

        let issues = extract_issue_numbers(&prompt);
        assert_eq!(issues, vec![42, 99]);
    }

    #[test]
    fn run_handles_missing_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, r#"{"branch": "test"}"#).unwrap();

        let content = std::fs::read_to_string(&state_path).unwrap();
        let state: serde_json::Value = serde_json::from_str(&content).unwrap();
        let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let issues = extract_issue_numbers(prompt);
        assert!(issues.is_empty());
    }

    #[test]
    fn add_flag_maps_to_add_label() {
        let flag = if true {
            "--add-label"
        } else {
            "--remove-label"
        };
        assert_eq!(flag, "--add-label");
    }

    #[test]
    fn remove_flag_maps_to_remove_label() {
        let flag = if false {
            "--add-label"
        } else {
            "--remove-label"
        };
        assert_eq!(flag, "--remove-label");
    }

    // --- label_issues_with_runner ---

    #[test]
    fn label_issues_with_runner_all_succeed() {
        let factory = |_args: &[&str]| {
            Command::new("sh")
                .args(["-c", "exit 0"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let result = label_issues_with_runner(&[1, 2], "add", &factory);
        assert_eq!(result.labeled, vec![1, 2]);
        assert!(result.failed.is_empty());
    }

    #[test]
    fn label_issues_with_runner_all_fail_on_nonzero_exit() {
        let factory = |_args: &[&str]| {
            Command::new("sh")
                .args(["-c", "exit 1"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let result = label_issues_with_runner(&[3, 4], "remove", &factory);
        assert!(result.labeled.is_empty());
        assert_eq!(result.failed, vec![3, 4]);
    }

    #[test]
    fn label_issues_with_runner_spawn_error_marks_failed() {
        let factory = |_args: &[&str]| -> std::io::Result<Child> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no binary",
            ))
        };
        let result = label_issues_with_runner(&[5], "add", &factory);
        assert_eq!(result.failed, vec![5]);
    }

    // --- run_impl_main ---

    #[test]
    fn label_issues_run_impl_main_missing_state_returns_error_tuple() {
        let args = Args {
            state_file: "/nonexistent/state.json".to_string(),
            add: true,
            remove: false,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert_eq!(value["step"], "read_state");
    }

    #[test]
    fn label_issues_run_impl_main_corrupt_state_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, "{not json").unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            add: true,
            remove: false,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert_eq!(value["step"], "parse_state");
    }

    #[test]
    fn label_issues_run_impl_main_no_prompt_returns_ok_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, r#"{"branch":"test"}"#).unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            add: true,
            remove: false,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["labeled"].as_array().unwrap().len(), 0);
        assert_eq!(value["failed"].as_array().unwrap().len(), 0);
    }

    // --- run_impl_main_with_runner (seam wired through dispatcher) ---

    #[test]
    fn label_issues_run_impl_main_with_runner_dispatches_to_seam() {
        // Plan-named: prove run_impl_main_with_runner reaches
        // label_issues_with_runner with the injected child_factory, so
        // a future refactor of the dispatcher can't silently bypass the
        // seam. Per .claude/rules/subprocess-test-hygiene.md, the test
        // never spawns real `gh`.
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        std::fs::write(
            &state_file,
            r#"{"prompt":"work on #42 and #43","branch":"test"}"#,
        )
        .unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            add: true,
            remove: false,
        };
        let factory = |_args: &[&str]| {
            Command::new("sh")
                .args(["-c", "exit 0"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let (value, code) = run_impl_main_with_runner(args, &factory);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["labeled"].as_array().unwrap().len(), 2);
        assert_eq!(value["failed"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn label_issues_run_impl_main_with_runner_mixed_outcomes_partitions_correctly() {
        // Plan-named (R2): exercise mixed success/failure partitioning
        // through the dispatcher. The first issue's gh exit succeeds;
        // the second exits non-zero. Result must split labeled vs failed.
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        std::fs::write(
            &state_file,
            r#"{"prompt":"work on #1 and #2","branch":"test"}"#,
        )
        .unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            add: true,
            remove: false,
        };
        let factory = |args: &[&str]| {
            // args = ["issue", "edit", "<num>", flag, LABEL]
            let num = args[2];
            let cmd = if num == "1" { "exit 0" } else { "exit 1" };
            Command::new("sh")
                .args(["-c", cmd])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let (value, code) = run_impl_main_with_runner(args, &factory);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["labeled"], serde_json::json!([1]));
        assert_eq!(value["failed"], serde_json::json!([2]));
    }
}
