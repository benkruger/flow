//! Close GitHub issues referenced in the FLOW start prompt.
//!
//! Reads the state file, extracts #N patterns from the prompt field,
//! and closes each issue via gh CLI after the PR is merged.
//!
//! Usage: bin/flow close-issues --state-file <path>
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "closed": [{"number": 83, "url": "..."}], "failed": [{"number": 89, "error": "not found"}]}

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
    let mut closed = Vec::new();
    let mut failed = Vec::new();
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

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

/// Close each issue via gh CLI. Returns closed and failed lists.
///
/// When repo is provided, closed items include URLs.
pub fn close_issues(issue_numbers: &[i64], repo: Option<&str>) -> (Vec<Value>, Vec<Value>) {
    close_issues_with_runner(issue_numbers, repo, &|args| {
        Command::new("gh")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
}

fn close_single_issue(
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
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = match child.wait_with_output() {
                    Ok(o) => o,
                    Err(e) => return Err(e.to_string()),
                };
                if output.status.success() {
                    return Ok(());
                }
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(stderr);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timeout".to_string());
                }
                std::thread::sleep(poll_interval.min(timeout - start.elapsed()));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Main-arm dispatcher with injected child_factory. Reads the state
/// file, extracts issue numbers, and calls `close_issues_with_runner`
/// with the injected factory. Tests pass a mock child_factory to
/// exercise the gh-spawning branch without spawning real `gh`.
/// Returns `(value, exit_code)` — `(error, 1)` on state-file read or
/// parse failure, `(ok+closed+failed, 0)` on success (the `failed`
/// list captures per-issue gh failures).
pub fn run_impl_main_with_runner(
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

pub fn run(args: Args) -> ! {
    let (value, code) = run_impl_main(args);
    crate::dispatch::dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- CLI integration: run() reads state file ---

    #[test]
    fn run_no_prompt_outputs_empty_lists() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        fs::write(&state_file, r#"{"branch": "test"}"#).unwrap();

        // Can't easily test run() because it calls process::exit and json_ok prints to stdout.
        // Verify the logic path instead: extract_issue_numbers on empty prompt.
        let issue_numbers = extract_issue_numbers("");
        assert!(issue_numbers.is_empty());

        let (closed, failed) = close_issues(&issue_numbers, None);
        assert!(closed.is_empty());
        assert!(failed.is_empty());
    }

    #[test]
    fn close_issues_empty_list() {
        let (closed, failed) = close_issues(&[], None);
        assert!(closed.is_empty());
        assert!(failed.is_empty());
    }

    // --- close_issues_with_runner ---

    #[test]
    fn close_issues_with_runner_all_succeed() {
        let factory = |_args: &[&str]| {
            Command::new("sh")
                .args(["-c", "exit 0"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let (closed, failed) = close_issues_with_runner(&[1, 2], Some("owner/repo"), &factory);
        assert_eq!(closed.len(), 2);
        assert!(failed.is_empty());
        assert_eq!(closed[0]["number"], 1);
        assert!(closed[0]["url"]
            .as_str()
            .unwrap()
            .contains("owner/repo/issues/1"));
    }

    #[test]
    fn close_issues_with_runner_partial_failure() {
        let factory = |args: &[&str]| {
            // First arg after "issue close" is the number; "1" succeeds, "2" fails.
            let num = args[2];
            let cmd = if num == "1" {
                "exit 0"
            } else {
                "echo nope 1>&2; exit 1"
            };
            Command::new("sh")
                .args(["-c", cmd])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        let (closed, failed) = close_issues_with_runner(&[1, 2], None, &factory);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0]["number"], 1);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["number"], 2);
        assert!(failed[0]["error"].as_str().unwrap().contains("nope"));
    }

    // --- run_impl_main ---

    #[test]
    fn close_issues_run_impl_main_no_state_returns_error_tuple() {
        let args = Args {
            state_file: "/nonexistent/state.json".to_string(),
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Could not read state file"));
    }

    #[test]
    fn close_issues_run_impl_main_corrupt_state_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        fs::write(&state_file, "{not json").unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
    }

    #[test]
    fn close_issues_run_impl_main_no_prompt_returns_empty_lists() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        fs::write(&state_file, r#"{"branch":"test"}"#).unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["closed"].as_array().unwrap().len(), 0);
        assert_eq!(value["failed"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn close_issues_with_runner_spawn_failure_returns_failed_entry() {
        // Drives the spawn-failure branch: factory returns Err → close_single_issue
        // returns Err("Failed to spawn: ...") → entry lands in `failed`.
        let factory = |_args: &[&str]| -> std::io::Result<Child> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such binary",
            ))
        };
        let (closed, failed) = close_issues_with_runner(&[42], None, &factory);
        assert!(closed.is_empty());
        assert_eq!(failed.len(), 1);
        assert!(failed[0]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to spawn"));
    }

    // --- run_impl_main_with_runner (seam wired through dispatcher) ---

    #[test]
    fn close_issues_run_impl_main_with_runner_dispatches_to_seam() {
        // Plan-named: prove run_impl_main_with_runner reaches
        // close_issues_with_runner with the injected child_factory, so
        // a future refactor of the dispatcher can't silently bypass the
        // seam. Per .claude/rules/subprocess-test-hygiene.md, the test
        // never spawns real `gh`.
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        fs::write(
            &state_file,
            r#"{"prompt":"work on #42 and #43","repo":"owner/repo"}"#,
        )
        .unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
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
        assert_eq!(value["closed"].as_array().unwrap().len(), 2);
        assert_eq!(value["failed"].as_array().unwrap().len(), 0);
    }
}
