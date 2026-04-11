//! Pre-merge freshness check.
//!
//! Fetches origin/main, verifies the branch is up-to-date via
//! `git merge-base --is-ancestor`, and merges if behind. Detects merge
//! conflicts via `git status --porcelain`. Tracks retries in the state
//! file (max 3) under the `mutate_state` lock.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::{LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::lock::mutate_state;
use crate::utils::parse_conflict_files;
const MAX_RETRIES: i64 = 3;

#[derive(Parser, Debug)]
#[command(name = "check-freshness", about = "Pre-merge freshness check")]
pub struct Args {
    /// Raw args — parsed manually inside run() to silently skip unknown
    /// flags matching Python's manual arg loop
    /// (tested by test_cli_unknown_args_ignored).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
    pub raw_args: Vec<String>,
}

/// Result of a single git subprocess call — either completed (with exit
/// code and captured output) or timed out.
#[derive(Debug, Clone)]
pub enum CmdResult {
    Ok {
        returncode: i32,
        stdout: String,
        stderr: String,
    },
    Timeout,
}

/// Pure check-freshness logic with injected command runner.
///
/// The `git_cmd` closure receives `(args, timeout_secs)` and returns a
/// [`CmdResult`]. Production code wraps [`run_git_cmd`] which calls real
/// git; tests provide mock closures returning pre-staged results.
pub fn check_freshness_impl(
    state_file: Option<&Path>,
    git_cmd: &mut dyn FnMut(&[&str], u64) -> CmdResult,
) -> Value {
    // Retry gate — if we've already retried MAX_RETRIES times, stop.
    if let Some(path) = state_file {
        let retries = read_retries(path);
        if retries >= MAX_RETRIES {
            return json!({"status": "max_retries", "retries": retries});
        }
    }

    // Step 1: git fetch origin main
    match git_cmd(&["git", "fetch", "origin", "main"], NETWORK_TIMEOUT) {
        CmdResult::Timeout => {
            return json!({
                "status": "error",
                "step": "fetch",
                "message": format!("git fetch timed out after {}s", NETWORK_TIMEOUT),
            });
        }
        CmdResult::Ok {
            returncode, stderr, ..
        } if returncode != 0 => {
            return json!({
                "status": "error",
                "step": "fetch",
                "message": stderr.trim(),
            });
        }
        _ => {}
    }

    // Step 2: git merge-base --is-ancestor origin/main HEAD
    // Exit 0 → up_to_date. Non-zero or timeout → proceed to merge attempt
    // (Python swallows the timeout intentionally — tested by
    // test_merge_base_timeout).
    let mb = git_cmd(
        &["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
        LOCAL_TIMEOUT,
    );
    if let CmdResult::Ok { returncode: 0, .. } = mb {
        return json!({"status": "up_to_date"});
    }

    // Step 3: git merge origin/main
    let merge_result = git_cmd(&["git", "merge", "origin/main"], NETWORK_TIMEOUT);
    match &merge_result {
        CmdResult::Timeout => {
            return json!({
                "status": "error",
                "step": "merge",
                "message": format!("git merge timed out after {}s", NETWORK_TIMEOUT),
            });
        }
        CmdResult::Ok { returncode: 0, .. } => {
            let mut out = json!({"status": "merged"});
            if let Some(path) = state_file {
                let retries = increment_retries(path);
                out["retries"] = json!(retries);
            }
            return out;
        }
        _ => {}
    }

    // Merge failed — capture its stderr for the fallthrough error message.
    let merge_stderr = match &merge_result {
        CmdResult::Ok { stderr, .. } => stderr.trim().to_string(),
        _ => String::new(),
    };

    // Step 4: git status --porcelain to detect conflicts.
    match git_cmd(&["git", "status", "--porcelain"], LOCAL_TIMEOUT) {
        CmdResult::Ok { stdout, .. } => {
            let files = parse_conflict_files(&stdout);
            if !files.is_empty() {
                let mut out = json!({"status": "conflict", "files": files});
                if let Some(path) = state_file {
                    let retries = increment_retries(path);
                    out["retries"] = json!(retries);
                }
                return out;
            }
        }
        CmdResult::Timeout => {
            return json!({
                "status": "error",
                "step": "merge",
                "message": merge_stderr,
            });
        }
    }

    // Merge failed without conflict markers — return the merge stderr.
    json!({
        "status": "error",
        "step": "merge",
        "message": merge_stderr,
    })
}

/// Read `freshness_retries` from the state file under the mutate_state lock.
/// Returns 0 if the key is missing, the value has an unexpected type,
/// or the state file is unreadable.
fn read_retries(state_path: &Path) -> i64 {
    let cell = std::cell::Cell::new(0i64);
    let _ = mutate_state(state_path, |state| {
        // State Mutation Object Guard — prevents panic on non-object values
        if !(state.is_object() || state.is_null()) {
            return;
        }
        cell.set(read_retries_value(state));
    });
    cell.get()
}

/// Increment `freshness_retries` in the state file atomically under lock.
/// Returns the new (incremented) value. Handles missing/malformed keys by
/// treating them as 0 before incrementing.
fn increment_retries(state_path: &Path) -> i64 {
    let cell = std::cell::Cell::new(0i64);
    let _ = mutate_state(state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let next = read_retries_value(state) + 1;
        state["freshness_retries"] = json!(next);
        cell.set(next);
    });
    cell.get()
}

/// Counter type tolerance — state files in the wild may store
/// `freshness_retries` as int, float, or string (older versions or hand
/// edits). Accepts all three representations for backwards compatibility.
fn read_retries_value(state: &Value) -> i64 {
    state
        .get("freshness_retries")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_f64().map(|f| f as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
        .unwrap_or(0)
}

/// Real git subprocess runner with polling-based timeout and thread-drain.
///
/// Uses the thread-drain pattern to prevent pipe buffer deadlock: take
/// stdout/stderr handles before the poll
/// loop, drain them in spawned reader threads, poll `try_wait()` for exit
/// status, then join the readers. Compliant reference: see
/// `src/analyze_issues.rs` lines 472-518.
fn run_git_cmd(args: &[&str], timeout_secs: u64, cwd: &Path) -> CmdResult {
    use std::io::Read;

    let mut child = match Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            // Spawn error — e.g. git binary missing. Map to returncode 127
            // so callers see a non-zero exit without panicking.
            return CmdResult::Ok {
                returncode: 127,
                stdout: String::new(),
                stderr: String::new(),
            };
        }
    };

    // Drain stdout/stderr in threads to prevent pipe buffer deadlock.
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_handle {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_reader = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr_handle {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Join readers even on timeout so they do not leak.
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return CmdResult::Timeout;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return CmdResult::Ok {
                    returncode: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    CmdResult::Ok {
        returncode: status.and_then(|s| s.code()).unwrap_or(-1),
        stdout,
        stderr,
    }
}

/// Production wrapper: calls [`check_freshness_impl`] with a closure that
/// runs real git commands in the given working directory.
pub fn check_freshness(state_file: Option<&Path>, cwd: &Path) -> Value {
    let mut git = |args: &[&str], timeout_secs: u64| run_git_cmd(args, timeout_secs, cwd);
    check_freshness_impl(state_file, &mut git)
}

/// CLI entry point. Parses `raw_args` manually (silently skipping
/// unknowns, matching Python's manual loop — tested by
/// `test_cli_unknown_args_ignored`), runs `check_freshness`, prints the
/// JSON result, and exits with status 1 for any result other than
/// `up_to_date` or `merged`.
pub fn run(args: Args) {
    let mut state_file: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.raw_args.len() {
        if args.raw_args[i] == "--state-file" && i + 1 < args.raw_args.len() {
            state_file = Some(PathBuf::from(&args.raw_args[i + 1]));
            i += 2;
        } else {
            i += 1;
        }
    }

    // Inherit CWD from the calling process — must match Python's behavior.
    // Python's `subprocess.run` calls in check-freshness.py pass no `cwd=`
    // argument, so git commands run in the shell's current directory (the
    // feature worktree when invoked from complete-merge.py). Using
    // `project_root()` here would return the MAIN repo root (the first
    // entry of `git worktree list --porcelain`), causing git commands to
    // run in the main worktree where HEAD=main — which would make
    // `git merge-base --is-ancestor origin/main HEAD` trivially succeed
    // and always return `up_to_date` regardless of the feature branch's
    // actual state.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = check_freshness(state_file.as_deref(), &cwd);

    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("error")
        .to_string();
    println!("{}", serde_json::to_string(&result).unwrap());

    if status != "up_to_date" && status != "merged" {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;

    // --- helpers ---

    /// Build a mock git command runner that replays a fixed list of
    /// responses in order, panicking on extra calls.
    fn mock_runner(responses: Vec<CmdResult>) -> impl FnMut(&[&str], u64) -> CmdResult {
        let mut iter = responses.into_iter();
        move |_args, _timeout| iter.next().expect("Unexpected extra git call")
    }

    /// CmdResult::Ok { returncode: 0, stdout: "", stderr: "" }
    fn ok() -> CmdResult {
        CmdResult::Ok {
            returncode: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// CmdResult::Ok { returncode, stdout: "", stderr }
    fn err(returncode: i32, stderr: &str) -> CmdResult {
        CmdResult::Ok {
            returncode,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    /// CmdResult::Ok { returncode: 0, stdout, stderr: "" }
    fn stdout_ok(stdout: &str) -> CmdResult {
        CmdResult::Ok {
            returncode: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn make_state_file(dir: &std::path::Path, retries: i64) -> PathBuf {
        let path = dir.join("state.json");
        fs::write(
            &path,
            json!({"branch": "test", "freshness_retries": retries}).to_string(),
        )
        .unwrap();
        path
    }

    // --- check_freshness_impl unit tests ---

    #[test]
    fn test_up_to_date() {
        let responses = vec![
            ok(), // git fetch origin main
            ok(), // git merge-base --is-ancestor (exit 0 = ancestor)
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(result, json!({"status": "up_to_date"}));
    }

    #[test]
    fn test_merged() {
        let responses = vec![
            ok(),                      // fetch
            err(1, ""),                // merge-base (not ancestor)
            stdout_ok("Merge made\n"), // merge
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(result, json!({"status": "merged"}));
    }

    #[test]
    fn test_conflict() {
        let responses = vec![
            ok(),                                    // fetch
            err(1, ""),                              // merge-base
            err(1, "CONFLICT"),                      // merge fails
            stdout_ok("UU file1.py\nAA file2.py\n"), // status
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({"status": "conflict", "files": ["file1.py", "file2.py"]})
        );
    }

    #[test]
    fn test_fetch_failure() {
        let responses = vec![err(1, "Could not resolve host")];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({
                "status": "error",
                "step": "fetch",
                "message": "Could not resolve host",
            })
        );
    }

    #[test]
    fn test_merge_error_non_conflict() {
        let responses = vec![
            ok(),                   // fetch
            err(1, ""),             // merge-base
            err(1, "merge failed"), // merge
            stdout_ok(""),          // status (clean)
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({
                "status": "error",
                "step": "merge",
                "message": "merge failed",
            })
        );
    }

    #[test]
    fn test_fetch_timeout() {
        let responses = vec![CmdResult::Timeout];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({
                "status": "error",
                "step": "fetch",
                "message": "git fetch timed out after 60s",
            })
        );
    }

    #[test]
    fn test_merge_base_timeout() {
        let responses = vec![
            ok(),                            // fetch
            CmdResult::Timeout,              // merge-base timeout
            stdout_ok("Already up to date"), // merge succeeds
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(result, json!({"status": "merged"}));
    }

    #[test]
    fn test_merge_timeout() {
        let responses = vec![
            ok(),               // fetch
            err(1, ""),         // merge-base
            CmdResult::Timeout, // merge timeout
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({
                "status": "error",
                "step": "merge",
                "message": "git merge timed out after 60s",
            })
        );
    }

    #[test]
    fn test_status_porcelain_timeout() {
        let responses = vec![
            ok(),               // fetch
            err(1, ""),         // merge-base
            err(1, "CONFLICT"), // merge fails
            CmdResult::Timeout, // status timeout
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(
            result,
            json!({
                "status": "error",
                "step": "merge",
                "message": "CONFLICT",
            })
        );
    }

    #[test]
    fn test_correct_git_commands_up_to_date() {
        let calls = RefCell::new(Vec::<(Vec<String>, u64)>::new());
        let responses = RefCell::new(vec![ok(), ok()].into_iter());
        let mut git = |args: &[&str], timeout: u64| -> CmdResult {
            calls
                .borrow_mut()
                .push((args.iter().map(|s| s.to_string()).collect(), timeout));
            responses.borrow_mut().next().unwrap()
        };
        check_freshness_impl(None, &mut git);
        let recorded = calls.into_inner();
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded[0].0,
            ["git", "fetch", "origin", "main"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(recorded[0].1, 60);
        assert_eq!(
            recorded[1].0,
            ["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(recorded[1].1, 30);
    }

    #[test]
    fn test_correct_git_commands_merged() {
        let calls = RefCell::new(Vec::<(Vec<String>, u64)>::new());
        let responses = RefCell::new(vec![ok(), err(1, ""), ok()].into_iter());
        let mut git = |args: &[&str], timeout: u64| -> CmdResult {
            calls
                .borrow_mut()
                .push((args.iter().map(|s| s.to_string()).collect(), timeout));
            responses.borrow_mut().next().unwrap()
        };
        check_freshness_impl(None, &mut git);
        let recorded = calls.into_inner();
        assert_eq!(recorded.len(), 3);
        assert_eq!(
            recorded[2].0,
            ["git", "merge", "origin/main"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(recorded[2].1, 60);
    }

    #[test]
    fn test_dd_conflict_detected() {
        let responses = vec![
            ok(),
            err(1, ""),
            err(1, "CONFLICT"),
            stdout_ok("DD deleted.py\n"),
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(result["status"], "conflict");
        assert_eq!(result["files"], json!(["deleted.py"]));
    }

    // --- retry tracking tests ---

    #[test]
    fn test_retry_increment() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = make_state_file(dir.path(), 0);
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&state_file), &mut git);
        assert_eq!(result, json!({"status": "merged", "retries": 1}));
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 1);
    }

    #[test]
    fn test_retry_max_reached() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = make_state_file(dir.path(), 3);
        // Closure should never be called — panic if it is.
        let mut git = |_args: &[&str], _timeout: u64| -> CmdResult {
            panic!("git_cmd should not be called at max retries");
        };
        let result = check_freshness_impl(Some(&state_file), &mut git);
        assert_eq!(result, json!({"status": "max_retries", "retries": 3}));
    }

    #[test]
    fn test_retry_no_state_file() {
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(None, &mut git);
        assert_eq!(result, json!({"status": "merged"}));
        assert!(result.get("retries").is_none());
    }

    #[test]
    fn test_retry_not_incremented_on_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = make_state_file(dir.path(), 1);
        let responses = vec![ok(), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&state_file), &mut git);
        assert_eq!(result, json!({"status": "up_to_date"}));
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 1);
    }

    #[test]
    fn test_retry_increment_on_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = make_state_file(dir.path(), 1);
        let responses = vec![
            ok(),
            err(1, ""),
            err(1, "CONFLICT"),
            stdout_ok("UU conflict.py\n"),
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&state_file), &mut git);
        assert_eq!(result["status"], "conflict");
        assert_eq!(result["retries"], 2);
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 2);
    }

    #[test]
    fn test_retry_missing_key_in_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, json!({"branch": "test"}).to_string()).unwrap();
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&path), &mut git);
        assert_eq!(result, json!({"status": "merged", "retries": 1}));
        let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 1);
    }

    // Counter type tolerance tests: the fallback chain in `read_retries_value`
    // accepts int, float, and string representations because state files can
    // outlive the code that writes them.

    #[test]
    fn test_retry_value_as_float() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Write freshness_retries as a float (e.g. from older Python code
        // that did float arithmetic on the counter).
        fs::write(&path, r#"{"branch":"test","freshness_retries":1.0}"#).unwrap();
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&path), &mut git);
        assert_eq!(result, json!({"status": "merged", "retries": 2}));
        let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 2);
    }

    #[test]
    fn test_retry_value_as_string() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Write freshness_retries as a JSON string (e.g. from a hand-edited
        // state file or a corrupted write).
        fs::write(&path, r#"{"branch":"test","freshness_retries":"2"}"#).unwrap();
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&path), &mut git);
        assert_eq!(result, json!({"status": "merged", "retries": 3}));
        let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state["freshness_retries"], 3);
    }

    #[test]
    fn test_retry_value_as_unparseable_string_defaults_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"branch":"test","freshness_retries":"garbage"}"#).unwrap();
        let responses = vec![ok(), err(1, ""), ok()];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&path), &mut git);
        // Unparseable string falls back to 0, then increments to 1.
        assert_eq!(result, json!({"status": "merged", "retries": 1}));
    }
}
