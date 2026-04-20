//! Pre-merge freshness check.
//!
//! Fetches origin/main, verifies the branch is up-to-date via
//! `git merge-base --is-ancestor`, and merges if behind. Detects merge
//! conflicts via `git status --porcelain`. Tracks retries in the state
//! file (max 3) under the `mutate_state` lock.
//!
//! Tests live at `tests/check_freshness.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

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
pub fn read_retries(state_path: &Path) -> i64 {
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
pub fn increment_retries(state_path: &Path) -> i64 {
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
pub fn run_git_cmd(args: &[&str], timeout_secs: u64, cwd: &Path) -> CmdResult {
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
pub fn exit_code_for_status(result: &Value) -> i32 {
    if result["status"] == "up_to_date" || result["status"] == "merged" {
        0
    } else {
        1
    }
}
