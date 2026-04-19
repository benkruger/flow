//! Pre-merge freshness check.
//!
//! Fetches origin/main, verifies the branch is up-to-date via
//! `git merge-base --is-ancestor`, and merges if behind. Detects merge
//! conflicts via `git status --porcelain`. Tracks retries in the state
//! file (max 3) under the `mutate_state` lock.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::complete_preflight::{LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::lock::mutate_state;
use crate::utils::{parse_conflict_files, tolerant_i64};
const MAX_RETRIES: i64 = 3;

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
    // Exit 0 → up_to_date. Non-zero or timeout → proceed to merge
    // attempt. Timeouts are deliberately swallowed (rather than
    // returned as an error) so a slow `git merge-base` falls through
    // to the merge step instead of aborting freshness — verified by
    // `test_merge_base_timeout`.
    let mb = git_cmd(
        &["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
        LOCAL_TIMEOUT,
    );
    if let CmdResult::Ok { returncode: 0, .. } = mb {
        return json!({"status": "up_to_date"});
    }

    // Step 3: git merge origin/main. The match is exhaustive across
    // every CmdResult variant — extracting `merge_stderr` from the
    // failing-Ok arm (returncode != 0) avoids the dead catch-all that
    // a separate post-match destructure would otherwise produce.
    let merge_stderr = match git_cmd(&["git", "merge", "origin/main"], NETWORK_TIMEOUT) {
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
        CmdResult::Ok { stderr, .. } => stderr.trim().to_string(),
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
        cell.set(tolerant_i64(&state["freshness_retries"]));
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
        let next = tolerant_i64(&state["freshness_retries"]).saturating_add(1);
        state["freshness_retries"] = json!(next);
        cell.set(next);
    });
    cell.get()
}

/// Real git subprocess runner with timeout enforcement.
///
/// Spawns a worker thread that runs [`Command::output()`] and sends the
/// result through a channel. The main thread waits on the channel with
/// [`mpsc::Receiver::recv_timeout`]; a timeout returns [`CmdResult::Timeout`]
/// and leaves the child to exit on its own (git's own network timeouts
/// eventually terminate fetch/merge).
fn run_git_cmd(args: &[&str], timeout_secs: u64, cwd: &Path) -> CmdResult {
    let (tx, rx) = mpsc::channel();
    let args_owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let cwd_owned = cwd.to_path_buf();
    thread::spawn(move || {
        let result = Command::new(&args_owned[0])
            .args(&args_owned[1..])
            .current_dir(&cwd_owned)
            .output();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Ok(output)) => CmdResult::Ok {
            returncode: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Ok(Err(_)) => CmdResult::Ok {
            returncode: 127,
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(_) => CmdResult::Timeout,
    }
}

/// Production wrapper: calls [`check_freshness_impl`] with a closure that
/// runs real git commands in the given working directory.
pub fn check_freshness(state_file: Option<&Path>, cwd: &Path) -> Value {
    let mut git = |args: &[&str], timeout_secs: u64| run_git_cmd(args, timeout_secs, cwd);
    check_freshness_impl(state_file, &mut git)
}

/// CLI entry point. Parses `raw_args` manually so unknown flags are
/// silently skipped (verified by `test_cli_unknown_args_ignored`),
/// runs `check_freshness`, and returns (JSON value, exit code) —
/// exit code 1 for any result other than `up_to_date` or `merged`.
///
/// Inherit CWD from the calling process — git commands must run in
/// the feature worktree (the shell's current directory), not the
/// main repo root.
pub fn run_impl_main(raw_args: &[String], cwd: &Path) -> (Value, i32) {
    let mut state_file: Option<PathBuf> = None;
    let mut i = 0;
    while i < raw_args.len() {
        if raw_args[i] == "--state-file" && i + 1 < raw_args.len() {
            state_file = Some(PathBuf::from(&raw_args[i + 1]));
            i += 2;
        } else {
            i += 1;
        }
    }

    let result = check_freshness(state_file.as_deref(), cwd);
    let code = exit_code_for_status(&result);
    (result, code)
}

/// Map a `check_freshness` result to an exit code: `up_to_date` and
/// `merged` → 0, everything else → 1.
fn exit_code_for_status(result: &Value) -> i32 {
    if result["status"] == "up_to_date" || result["status"] == "merged" {
        0
    } else {
        1
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
        // At max retries, check_freshness_impl must return early
        // before any git call. `mock_runner(vec![])` would panic on
        // first invocation with "Unexpected extra git call" — so if
        // the early-return invariant ever breaks, this test fails
        // with a panic rather than silently passing.
        let dir = tempfile::tempdir().unwrap();
        let state_file = make_state_file(dir.path(), 3);
        let mut git = mock_runner(vec![]);
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

    /// Exercises lines 161, 175 — the `is_object() || is_null()`
    /// early-return guards inside `read_retries` and `increment_retries`.
    /// An array-root state file makes both guards fire, so both helpers
    /// return 0. The freshness flow continues to merge → increment, and
    /// the final "merged" payload reports retries=0 because increment
    /// noop-ed under the guard.
    #[test]
    fn test_retry_array_root_state_skips_read_and_increment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "[1, 2, 3]").unwrap();
        let responses = vec![
            ok(),       // fetch
            err(1, ""), // merge-base (not ancestor)
            ok(),       // merge succeeds
        ];
        let mut git = mock_runner(responses);
        let result = check_freshness_impl(Some(&path), &mut git);
        assert_eq!(result, json!({"status": "merged", "retries": 0}));
        // State file is still an array (mutate_state writes back unchanged).
        let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(after.is_array(), "Root should still be an array");
        assert_eq!(after.as_array().unwrap().len(), 3);
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

    // Counter type tolerance tests: `tolerant_i64` (imported from utils)
    // accepts int, float, and string representations for `freshness_retries`
    // because state files can outlive the code that writes them.

    #[test]
    fn test_retry_value_as_float() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Write freshness_retries as a float to verify the reader's
        // type-tolerance fallback chain accepts a JSON number that
        // arrived as `1.0` instead of `1`.
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

    // --- run_git_cmd integration: real subprocess via /usr/bin/true, /usr/bin/false, and a missing binary ---

    /// Unwrap a `CmdResult::Ok` into (returncode, stdout, stderr) or
    /// panic with the actual variant. Both arms are exercised because
    /// some tests expect Ok (true arm) and others expect Timeout (use
    /// `expect_timeout` below).
    fn expect_ok(result: CmdResult) -> (i32, String, String) {
        match result {
            CmdResult::Ok {
                returncode,
                stdout,
                stderr,
            } => (returncode, stdout, stderr),
            CmdResult::Timeout => panic!("expected Ok, got Timeout"),
        }
    }

    /// Unwrap a `CmdResult::Timeout` or panic with the actual variant.
    /// Complements `expect_ok` so both CmdResult arms are covered.
    fn expect_timeout(result: CmdResult) {
        match result {
            CmdResult::Timeout => {}
            CmdResult::Ok {
                returncode,
                stdout,
                stderr,
            } => panic!(
                "expected Timeout, got Ok(returncode={}, stdout={:?}, stderr={:?})",
                returncode, stdout, stderr
            ),
        }
    }

    #[test]
    fn run_git_cmd_success_returns_returncode_zero() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let (code, _, _) = expect_ok(run_git_cmd(&["/usr/bin/true"], 5, &cwd));
        assert_eq!(code, 0);
    }

    #[test]
    fn run_git_cmd_nonzero_exit_returns_returncode() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let (code, _, _) = expect_ok(run_git_cmd(&["/usr/bin/false"], 5, &cwd));
        assert_eq!(code, 1);
    }

    #[test]
    fn run_git_cmd_spawn_failure_returns_127() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let (code, _, _) = expect_ok(run_git_cmd(
            &["/no/such/binary/here-deadbeef"],
            5,
            &cwd,
        ));
        assert_eq!(code, 127);
    }

    #[test]
    fn run_git_cmd_timeout_returns_timeout_variant() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        expect_timeout(run_git_cmd(&["/bin/sleep", "10"], 1, &cwd));
    }

    #[test]
    fn run_git_cmd_signal_killed_returns_negative_one() {
        // Process self-kills via SIGKILL — ExitStatus::code() returns
        // None, which the `unwrap_or(-1)` fallback arm converts to -1.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let (code, _, _) = expect_ok(run_git_cmd(
            &["/bin/sh", "-c", "kill -9 $$"],
            5,
            &cwd,
        ));
        assert_eq!(code, -1);
    }

    #[test]
    fn run_git_cmd_captures_stdout_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let (code, stdout, stderr) = expect_ok(run_git_cmd(
            &["/bin/sh", "-c", "echo hi; echo err >&2"],
            5,
            &cwd,
        ));
        assert_eq!(code, 0);
        assert!(stdout.contains("hi"));
        assert!(stderr.contains("err"));
    }

    /// Ensure both panic-arms of expect_ok / expect_timeout fire —
    /// otherwise coverage would not reach them.
    #[test]
    #[should_panic(expected = "expected Ok")]
    fn expect_ok_panics_on_timeout() {
        expect_ok(CmdResult::Timeout);
    }

    #[test]
    #[should_panic(expected = "Unexpected extra git call")]
    fn mock_runner_panics_when_responses_exhausted() {
        let mut git = mock_runner(vec![ok()]);
        git(&[], 0);
        git(&[], 0);
    }

    #[test]
    #[should_panic(expected = "expected Timeout")]
    fn expect_timeout_panics_on_ok() {
        expect_timeout(CmdResult::Ok {
            returncode: 0,
            stdout: String::new(),
            stderr: String::new(),
        });
    }

    // --- check_freshness production wrapper ---

    #[test]
    fn check_freshness_production_wrapper_returns_json_object() {
        // Production check_freshness wraps check_freshness_impl with a
        // run_git_cmd closure. Verifies the wrapper produces a JSON
        // object with a status field (specific status varies by host
        // git availability).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let result = check_freshness(None, &cwd);
        assert!(result.is_object());
        assert!(result.get("status").is_some());
    }

    // --- run_impl_main CLI entry point ---

    #[test]
    fn run_impl_main_without_state_file_arg_parses_no_state() {
        // No `--state-file` arg → state_file is None. cwd points to an
        // empty tempdir so git fetch fails → non-zero exit code.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let raw_args: Vec<String> = Vec::new();
        let (value, code) = run_impl_main(&raw_args, &cwd);
        assert_eq!(code, 1);
        assert!(value.is_object());
    }

    #[test]
    fn run_impl_main_with_state_file_arg_parses_path() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let state_path = cwd.join("state.json");
        fs::write(&state_path, json!({"branch": "test"}).to_string()).unwrap();
        let raw_args = vec![
            "--state-file".to_string(),
            state_path.to_string_lossy().to_string(),
        ];
        let (_value, _code) = run_impl_main(&raw_args, &cwd);
        // Success is that run_impl_main returns without panicking;
        // the inner outcome depends on host git availability.
    }

    #[test]
    fn run_impl_main_ignores_unknown_flags() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let raw_args = vec!["--unknown-flag".to_string(), "ignored".to_string()];
        let (_value, _code) = run_impl_main(&raw_args, &cwd);
        // Unknown flags skipped over without clap-style parse errors.
    }

    #[test]
    fn exit_code_for_status_up_to_date_returns_zero() {
        assert_eq!(exit_code_for_status(&json!({"status": "up_to_date"})), 0);
    }

    #[test]
    fn exit_code_for_status_merged_returns_zero() {
        assert_eq!(exit_code_for_status(&json!({"status": "merged"})), 0);
    }

    #[test]
    fn exit_code_for_status_error_returns_one() {
        assert_eq!(exit_code_for_status(&json!({"status": "error"})), 1);
    }

    #[test]
    fn exit_code_for_status_missing_status_returns_one() {
        assert_eq!(exit_code_for_status(&json!({})), 1);
    }

    #[test]
    fn run_impl_main_state_file_arg_without_value_is_ignored() {
        // `--state-file` at end of args with no following value → loop
        // advances by 1, not 2 (no out-of-bounds).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let raw_args = vec!["--state-file".to_string()];
        let (_value, _code) = run_impl_main(&raw_args, &cwd);
    }
}
