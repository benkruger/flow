//! Pre-merge freshness check.
//!
//! Fetches origin/main, verifies the branch is up-to-date via
//! `git merge-base --is-ancestor`, and merges if behind. Detects merge
//! conflicts via `git status --porcelain`. Tracks retries in the state
//! file (max 3) under the `mutate_state` lock.
//!
//! Uses `complete_preflight::run_cmd_with_timeout` (shared with the
//! Complete phase) for git subprocess calls so timeout policy stays
//! consolidated in one place. Spawn-failure and timeout errors from the
//! shared runner are collapsed into a synthetic non-zero exit tuple so
//! every step's failure path is a single branch.
//!
//! Tests live at `tests/check_freshness.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::complete_preflight::{run_cmd_with_timeout, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::lock::mutate_state;
use crate::utils::{parse_conflict_files, tolerant_i64};

const MAX_RETRIES: i64 = 3;

/// Read `freshness_retries` from the state file under the mutate_state lock.
/// Returns 0 if the key is missing, the value has an unexpected type,
/// or the state file is unreadable.
fn read_retries(state_path: &Path) -> i64 {
    let cell = std::cell::Cell::new(0i64);
    let _ = mutate_state(state_path, &mut |state| {
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
    let _ = mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let next = tolerant_i64(&state["freshness_retries"]).saturating_add(1);
        state["freshness_retries"] = json!(next);
        cell.set(next);
    });
    cell.get()
}

/// Run `git -C <cwd> <args>` via the shared run_cmd_with_timeout. Spawn
/// failure and timeout Err variants are folded into a synthetic
/// `(-1, "", err_msg)` tuple so every caller can treat any non-zero
/// status as failure with a single branch.
fn run_git(args: &[&str], timeout_secs: u64, cwd: &Path) -> (i32, String, String) {
    let mut with_c: Vec<&str> = Vec::with_capacity(args.len() + 2);
    with_c.push("git");
    with_c.push("-C");
    let cwd_str = cwd.to_str().unwrap_or(".");
    with_c.push(cwd_str);
    for a in &args[1..] {
        with_c.push(a);
    }
    match run_cmd_with_timeout(&with_c, timeout_secs) {
        Ok(r) => r,
        Err(msg) => (-1, String::new(), msg),
    }
}

/// Core freshness logic: fetch, check ancestry, merge if behind.
///
/// `base_branch` is the integration branch (read from the state
/// file by the caller); the function targets `origin/<base_branch>`
/// for fetch/merge-base/merge so a repo whose default branch is
/// `staging` exercises the right remote ref instead of the
/// hardcoded `main`.
fn check_freshness(state_file: Option<&Path>, cwd: &Path, base_branch: &str) -> Value {
    if let Some(path) = state_file {
        let retries = read_retries(path);
        if retries >= MAX_RETRIES {
            return json!({"status": "max_retries", "retries": retries});
        }
    }

    let origin_ref = format!("origin/{}", base_branch);

    let (code, _, stderr) = run_git(
        &["git", "fetch", "origin", base_branch],
        NETWORK_TIMEOUT,
        cwd,
    );
    if code != 0 {
        return json!({
            "status": "error",
            "step": "fetch",
            "message": stderr.trim(),
        });
    }

    let (mb_code, _, _) = run_git(
        &["git", "merge-base", "--is-ancestor", &origin_ref, "HEAD"],
        LOCAL_TIMEOUT,
        cwd,
    );
    if mb_code == 0 {
        return json!({"status": "up_to_date"});
    }

    let (merge_code, _, merge_stderr) =
        run_git(&["git", "merge", &origin_ref], NETWORK_TIMEOUT, cwd);
    if merge_code == 0 {
        let mut out = json!({"status": "merged"});
        if let Some(path) = state_file {
            let retries = increment_retries(path);
            out["retries"] = json!(retries);
        }
        return out;
    }
    let merge_stderr = merge_stderr.trim().to_string();

    let (_, stdout, _) = run_git(&["git", "status", "--porcelain"], LOCAL_TIMEOUT, cwd);
    let files = parse_conflict_files(&stdout);
    if !files.is_empty() {
        let mut out = json!({"status": "conflict", "files": files});
        if let Some(path) = state_file {
            let retries = increment_retries(path);
            out["retries"] = json!(retries);
        }
        return out;
    }

    json!({
        "status": "error",
        "step": "merge",
        "message": merge_stderr,
    })
}

fn exit_code_for_status(result: &Value) -> i32 {
    if result["status"] == "up_to_date" || result["status"] == "merged" {
        0
    } else {
        1
    }
}

/// CLI entry point. Parses `raw_args` manually so unknown flags are
/// silently skipped, runs `check_freshness`, and returns
/// (JSON value, exit code) — exit code 1 for any result other than
/// `up_to_date` or `merged`.
///
/// Inherits CWD from the calling process — git commands run in the
/// feature worktree (the shell's current directory), not the main
/// repo root.
///
/// When `--state-file` is provided and the state file declares
/// `base_branch`, the freshness check targets `origin/<base_branch>`;
/// otherwise it falls back to `"main"` so legacy state files (and
/// the no-state-file invocation path) preserve pre-existing
/// behavior.
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

    let base_branch = state_file
        .as_deref()
        .and_then(|p| crate::git::read_base_branch(p).ok())
        .unwrap_or_else(|| "main".to_string());

    let result = check_freshness(state_file.as_deref(), cwd, &base_branch);
    let code = exit_code_for_status(&result);
    (result, code)
}
