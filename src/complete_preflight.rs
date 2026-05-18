//! Complete phase preflight — shared functions and standalone subcommand.
//!
//! Provides `resolve_mode`, `check_learn_phase`, `check_pr_status`,
//! `merge_main`, and `run_cmd_with_timeout` — reused by `complete-fast`
//! and available as a standalone subcommand for backward compatibility.
//!
//! Usage: bin/flow complete-preflight [--branch <name>] [--auto] [--manual]
//!
//! Output (JSON to stdout):
//!   Success:  {"status": "ok", "mode": "auto", "pr_state": "OPEN", "merge": "clean", "warnings": []}
//!   Merged:   {"status": "ok", "pr_state": "MERGED", ...}
//!   Conflict: {"status": "conflict", "conflict_files": ["..."], ...}
//!   Inferred: {"status": "ok", "inferred": true, ...}
//!   Error:    {"status": "error", "message": "..."}
//!
//! Tests live in `tests/complete_preflight.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
use crate::utils::{bin_flow_path, derive_worktree, parse_conflict_files};

/// Standard timeout for local subprocess calls (git status, git add, etc.).
pub const LOCAL_TIMEOUT: u64 = 30;
/// Standard timeout for network subprocess calls (git fetch, gh api, etc.).
pub const NETWORK_TIMEOUT: u64 = 60;
/// Step counter total for complete phase.
pub const COMPLETE_STEPS_TOTAL: i64 = 6;

pub type CmdResult = Result<(i32, String, String), String>;

/// Fold a `CmdResult` (the return type of `run_cmd_with_timeout`) into
/// a uniform `(exit_code, stdout, stderr)` tuple so every caller can
/// handle subprocess timeouts and spawn failures via the existing
/// non-zero-exit branch instead of a panic.
///
/// On `Err`, the result is `(-1, "", msg)` — the synthetic exit code
/// `-1` flags the failure for downstream `code != 0` checks and the
/// timeout/spawn-failure message lands in stderr's position so
/// existing error envelopes carry useful diagnostic content.
///
/// Per `.claude/rules/external-input-path-construction.md` "No
/// `.expect()` on Filesystem Reads in Hooks or CLI Subcommands":
/// callers must use a non-panicking fold rather than `.expect()`
/// because the upstream `default_branch_in` probe only proves
/// spawn-time availability at one moment; subsequent calls can
/// still time out under slow network conditions.
pub fn fold_cmd_result(r: CmdResult) -> (i32, String, String) {
    match r {
        Ok(t) => t,
        Err(msg) => (-1, String::new(), msg),
    }
}

#[derive(Parser, Debug)]
#[command(name = "complete-preflight", about = "FLOW Complete phase preflight")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
    /// Force auto mode
    #[arg(long)]
    pub auto: bool,
    /// Force manual mode
    #[arg(long)]
    pub manual: bool,
}

/// Run a subprocess command with a timeout. `args[0]` is the program.
///
/// Drains stdout and stderr in spawned threads to prevent pipe buffer
/// deadlock.
pub fn run_cmd_with_timeout(args: &[&str], timeout_secs: u64) -> CmdResult {
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

    let mut stdout_handle = child.stdout.take().expect("child stdout was piped above");
    let mut stderr_handle = child.stderr.take().expect("child stderr was piped above");
    let stdout_reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stdout_handle.read_to_string(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stderr_handle.read_to_string(&mut buf);
        buf
    });

    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);
    let status = loop {
        // try_wait() on a live child returns an I/O error only under
        // OS-level pathology; treated as a programmer invariant per
        // `.claude/rules/testability-means-simplicity.md`.
        let maybe_status = child
            .try_wait()
            .expect("try_wait on a live child cannot fail in practice");
        match maybe_status {
            Some(s) => break s,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(format!("Timed out after {}s", timeout_secs));
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                std::thread::sleep(poll_interval.min(remaining));
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let code = status.code().unwrap_or(1);
    Ok((code, stdout, stderr))
}

/// Resolve mode from flags and state file.
///
/// Priority: `--auto` > `--manual` > the state file's
/// `skills.flow-complete` entry > the conservative fallback. The
/// state-file read delegates to [`crate::resolve_skill_mode::resolve`],
/// which tolerates every config shape, normalizes the value, and
/// clamps it to `{auto, manual}`. When no state file was found, the
/// same `FALLBACK_MODE` ("manual") applies — the safe default the
/// terminal skills use before the irreversible Complete merge.
pub fn resolve_mode(auto: bool, manual: bool, state: Option<&Value>) -> String {
    if auto {
        return "auto".to_string();
    }
    if manual {
        return "manual".to_string();
    }
    match state {
        Some(st) => crate::resolve_skill_mode::resolve(st, "flow-complete"),
        None => crate::resolve_skill_mode::FALLBACK_MODE.to_string(),
    }
}

/// Check if Learn phase is complete. Returns list of warning strings.
pub fn check_learn_phase(state: &Value) -> Vec<String> {
    let learn_status = state
        .get("phases")
        .and_then(|p| p.get("flow-learn"))
        .and_then(|l| l.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");
    if learn_status != "complete" {
        vec![format!("Phase 5 not complete (status: {}).", learn_status)]
    } else {
        Vec::new()
    }
}

/// Check PR state via `gh pr view`. Returns PR state string on success.
pub fn check_pr_status(pr_number: Option<i64>, branch: &str) -> Result<String, String> {
    let identifier = if let Some(n) = pr_number {
        n.to_string()
    } else if !branch.is_empty() {
        branch.to_string()
    } else {
        return Err("No PR number or branch to check".to_string());
    };
    let (code, stdout, stderr) = run_cmd_with_timeout(
        &[
            "gh",
            "pr",
            "view",
            &identifier,
            "--json",
            "state",
            "--jq",
            ".state",
        ],
        NETWORK_TIMEOUT,
    )?;
    if code != 0 {
        let err = stderr.trim();
        if err.is_empty() {
            Err("Could not find PR".to_string())
        } else {
            Err(err.to_string())
        }
    } else {
        Ok(stdout.trim().to_string())
    }
}

/// Merge `origin/<base_branch>` into the current branch.
///
/// `base_branch` is the integration branch the flow coordinates
/// against (resolved by the caller via `git::default_branch_in`);
/// a repo whose default branch is `staging` passes `"staging"`
/// here so `git fetch / merge-base / merge` operate on the correct
/// remote ref instead of the hardcoded `main`.
///
/// Every `run_cmd_with_timeout` call folds `Err` (timeout OR spawn
/// failure) into `("error", Some(json!(msg)))` instead of
/// panicking. The upstream `default_branch_in` probe proves
/// spawn-time availability at one moment, but subsequent calls
/// (NETWORK_TIMEOUT on fetch/merge/push, LOCAL_TIMEOUT on
/// merge-base/status) can still time out on slow networks.
///
/// Returns one of:
///   ("clean", None) — already up to date
///   ("merged", None) — merged successfully (new commits)
///   ("conflict", Some(files_array)) — merge conflicts
///   ("error", Some(message_string)) — unexpected error
pub fn merge_main(base_branch: &str) -> (String, Option<Value>) {
    let origin_ref = format!("origin/{}", base_branch);
    // Every subprocess call routes through `fold_cmd_result`, which
    // folds `Err` (timeout OR spawn failure) into a synthetic
    // `(-1, "", msg)` tuple. Downstream `code != 0` checks then
    // produce structured error envelopes for both the timeout/
    // spawn-failure case and git's own non-zero-exit case, without
    // panicking. Per `.claude/rules/external-input-path-construction.md`
    // "No `.expect()` on Filesystem Reads in Hooks or CLI
    // Subcommands": the upstream `default_branch_in` probe proves
    // spawn-time availability at one moment, but subsequent calls
    // (NETWORK_TIMEOUT on fetch/merge/push, LOCAL_TIMEOUT on
    // merge-base/status) can still time out on slow networks.
    let (fetch_code, _, fetch_stderr) = fold_cmd_result(run_cmd_with_timeout(
        &["git", "fetch", "origin", base_branch],
        NETWORK_TIMEOUT,
    ));
    if fetch_code != 0 {
        return ("error".to_string(), Some(json!(fetch_stderr.trim())));
    }

    let (mb_code, _, _) = fold_cmd_result(run_cmd_with_timeout(
        &["git", "merge-base", "--is-ancestor", &origin_ref, "HEAD"],
        LOCAL_TIMEOUT,
    ));
    if mb_code == 0 {
        return ("clean".to_string(), None);
    }

    let (m_code, _, m_stderr) = fold_cmd_result(run_cmd_with_timeout(
        &["git", "merge", &origin_ref],
        NETWORK_TIMEOUT,
    ));
    if m_code == 0 {
        let (push_code, _, push_stderr) =
            fold_cmd_result(run_cmd_with_timeout(&["git", "push"], NETWORK_TIMEOUT));
        if push_code != 0 {
            (
                "error".to_string(),
                Some(json!(format!(
                    "Merge succeeded but push failed: {}",
                    push_stderr.trim()
                ))),
            )
        } else {
            ("merged".to_string(), None)
        }
    } else {
        let (_, status_stdout, _) = fold_cmd_result(run_cmd_with_timeout(
            &["git", "status", "--porcelain"],
            LOCAL_TIMEOUT,
        ));
        let conflicts = parse_conflict_files(&status_stdout);
        if !conflicts.is_empty() {
            ("conflict".to_string(), Some(json!(conflicts)))
        } else {
            ("error".to_string(), Some(json!(m_stderr.trim())))
        }
    }
}

/// Call phase-transition --action enter. Returns parsed JSON value on
/// success, error message on failure.
fn phase_transition_enter(branch: &str) -> Result<Value, String> {
    let bin_flow = bin_flow_path();
    let (code, stdout, stderr) = run_cmd_with_timeout(
        &[
            &bin_flow,
            "phase-transition",
            "--phase",
            "flow-complete",
            "--action",
            "enter",
            "--branch",
            branch,
        ],
        LOCAL_TIMEOUT,
    )?;
    if code != 0 {
        return Err(stderr.trim().to_string());
    }
    serde_json::from_str(stdout.trim())
        .map_err(|_| format!("Invalid JSON from phase-transition: {}", stdout))
}

fn preflight(branch: Option<&str>, auto: bool, manual: bool, root: &Path) -> Value {
    // Resolve branch
    let branch = match branch {
        Some(b) if !b.is_empty() => b.to_string(),
        _ => {
            return json!({
                "status": "error",
                "message": "Could not determine current branch"
            });
        }
    };

    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(paths) => paths.state_file(),
        None => {
            return json!({
                "status": "error",
                "message": format!(
                    "Branch '{}' is not a valid FLOW branch (contains '/' or is empty). \
                     FLOW state files use a flat layout that cannot address slash-containing \
                     branches; resume the flow in its canonical branch name.",
                    branch
                )
            });
        }
    };
    let mut state: Option<Value> = None;
    let mut inferred = false;

    if state_path.exists() {
        match std::fs::read_to_string(&state_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(v) => state = Some(v),
                Err(_) => {
                    return json!({
                        "status": "error",
                        "message": format!("Could not parse state file: {}", state_path.display())
                    });
                }
            },
            Err(e) => {
                return json!({
                    "status": "error",
                    "message": format!("Could not read state file: {}", e)
                });
            }
        }
    } else {
        inferred = true;
    }

    let mode = resolve_mode(auto, manual, state.as_ref());

    let warnings = match state.as_ref() {
        Some(s) => check_learn_phase(s),
        None => Vec::new(),
    };

    // Phase transition enter (only if state file exists)
    if state.is_some() {
        if let Err(e) = phase_transition_enter(&branch) {
            return json!({
                "status": "error",
                "message": format!("Phase transition failed: {}", e)
            });
        }

        // Set step counters. state_path points at a file read_to_string
        // already validated; no other writer in flow.
        let _ = mutate_state(&state_path, &mut |s| {
            s["complete_steps_total"] = json!(COMPLETE_STEPS_TOTAL);
            s["complete_step"] = json!(1);
        });
    }

    let pr_number = state
        .as_ref()
        .and_then(|s| s.get("pr_number"))
        .and_then(|v| v.as_i64());
    let pr_state = match check_pr_status(pr_number, &branch) {
        Ok(s) => s,
        Err(e) => {
            return json!({"status": "error", "message": e});
        }
    };

    let mut base = serde_json::Map::new();
    base.insert("mode".to_string(), json!(mode));
    base.insert("pr_state".to_string(), json!(pr_state));
    base.insert("warnings".to_string(), json!(warnings));
    base.insert("branch".to_string(), json!(branch));
    if inferred {
        base.insert("inferred".to_string(), json!(true));
    }
    if let Some(ref s) = state {
        base.insert("pr_number".to_string(), json!(pr_number));
        let pr_url = s.get("pr_url").and_then(|v| v.as_str()).unwrap_or("");
        base.insert("pr_url".to_string(), json!(pr_url));
        base.insert("worktree".to_string(), json!(derive_worktree(&branch)));
    }

    match pr_state.as_str() {
        "MERGED" => {
            let mut out = serde_json::Map::new();
            out.insert("status".to_string(), json!("ok"));
            for (k, v) in base {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        "CLOSED" => {
            let mut out = serde_json::Map::new();
            out.insert("status".to_string(), json!("error"));
            out.insert(
                "message".to_string(),
                json!("PR is closed but not merged. Reopen or create a new PR first."),
            );
            for (k, v) in base {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        "OPEN" => {
            // Resolve the integration branch from git (single source of
            // truth). Fail closed via the error envelope when git cannot
            // resolve it — `complete-preflight` cannot guess at the
            // merge target.
            let base_branch = match crate::git::default_branch_in(root) {
                Ok(b) => b,
                Err(msg) => {
                    let mut out = serde_json::Map::new();
                    out.insert("status".to_string(), json!("error"));
                    out.insert("message".to_string(), json!(msg));
                    for (k, v) in base {
                        out.insert(k, v);
                    }
                    return Value::Object(out);
                }
            };
            let (merge_status, merge_data) = merge_main(&base_branch);
            let mut out = serde_json::Map::new();
            match merge_status.as_str() {
                "conflict" => {
                    out.insert("status".to_string(), json!("conflict"));
                    out.insert(
                        "conflict_files".to_string(),
                        merge_data.unwrap_or(json!([])),
                    );
                    for (k, v) in base {
                        out.insert(k, v);
                    }
                }
                "error" => {
                    out.insert("status".to_string(), json!("error"));
                    out.insert("message".to_string(), merge_data.unwrap_or(json!("")));
                    for (k, v) in base {
                        out.insert(k, v);
                    }
                }
                _ => {
                    out.insert("status".to_string(), json!("ok"));
                    for (k, v) in base {
                        out.insert(k, v);
                    }
                    out.insert("merge".to_string(), json!(merge_status));
                }
            }
            Value::Object(out)
        }
        _ => {
            let mut out = serde_json::Map::new();
            out.insert("status".to_string(), json!("error"));
            out.insert(
                "message".to_string(),
                json!(format!("Unexpected PR state: {}", pr_state)),
            );
            for (k, v) in base {
                out.insert(k, v);
            }
            Value::Object(out)
        }
    }
}

/// Main-arm dispatch: returns (value, exit code).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let root = project_root();
    let resolved_branch: Option<String> = match args.branch.as_deref() {
        Some(b) => Some(b.to_string()),
        None => current_branch(),
    };
    let result = preflight(resolved_branch.as_deref(), args.auto, args.manual, &root);
    let code = if result["status"] == "ok" { 0 } else { 1 };
    (result, code)
}
