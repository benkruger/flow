//! Complete phase preflight — shared functions and standalone subcommand.
//!
//! Provides `resolve_mode`, `check_learn_phase`, `check_pr_status`,
//! `merge_main`, and `run_cmd_with_timeout` — reused by `complete-fast`
//! (the skill's Step 1 entry point) and available as a standalone
//! subcommand for backward compatibility.
//!
//! Usage: bin/flow complete-preflight [--branch <name>] [--auto] [--manual]
//!
//! Output (JSON to stdout):
//!   Success:  {"status": "ok", "mode": "auto", "pr_state": "OPEN", "merge": "clean", "warnings": []}
//!   Merged:   {"status": "ok", "pr_state": "MERGED", ...}
//!   Conflict: {"status": "conflict", "conflict_files": ["..."], ...}
//!   Inferred: {"status": "ok", "inferred": true, ...}
//!   Error:    {"status": "error", "message": "..."}

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
/// Step counter total for complete phase: 6 steps (running checks, local CI,
/// GitHub CI, confirming, merging PR, finalizing).
pub const COMPLETE_STEPS_TOTAL: i64 = 6;

pub type CmdResult = Result<(i32, String, String), String>;

/// Error returned by `wait_with_timeout` when the deadline expires
/// before the child exits.
///
/// `try_wait()` returning an OS-level I/O error on a live child is a
/// vanishingly rare case that has never been observed in practice;
/// `wait_with_timeout` treats it as a programmer invariant failure
/// via `.expect()` rather than widening the error surface with a
/// variant that production code would only hit under pathological
/// OS conditions.
#[derive(Debug)]
pub enum WaitError {
    Timeout,
}

/// Poll a `try_wait`-style function until the child is ready or the
/// deadline expires. Abstracted from `run_cmd_with_timeout`'s poll
/// loop so unit tests can inject mock `try_wait_fn` and `sleep_fn`
/// closures to cover every branch (immediate ready, polled then
/// exits, deadline expired) without spawning real subprocesses or
/// sleeping real wall-clock time.
///
/// The caller retains ownership of the child process. On
/// `Err(WaitError::Timeout)` the caller is responsible for calling
/// `child.kill()` + `child.wait()` and draining any stdio threads.
pub fn wait_with_timeout<W, S>(
    mut try_wait_fn: W,
    mut sleep_fn: S,
    timeout: Duration,
) -> Result<std::process::ExitStatus, WaitError>
where
    W: FnMut() -> std::io::Result<Option<std::process::ExitStatus>>,
    S: FnMut(Duration),
{
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);
    loop {
        // try_wait() on a live child returns an I/O error only under
        // OS-level pathology; treated as a programmer invariant.
        let status = try_wait_fn().expect("try_wait on a live child cannot fail in practice");
        match status {
            Some(s) => return Ok(s),
            None => {
                if start.elapsed() >= timeout {
                    return Err(WaitError::Timeout);
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                sleep_fn(poll_interval.min(remaining));
            }
        }
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
/// deadlock — children writing >64KB to a piped stream would otherwise
/// block forever when the kernel buffer fills and `try_wait()` would
/// never observe the child exiting.
///
/// Delegates the `try_wait` + sleep loop to `wait_with_timeout` and
/// dispatches the `WaitError` variants: `Timeout` triggers child
/// termination + stdio drain + formatted timeout error;
/// `Io(msg)` drains stdio and surfaces the message verbatim.
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

    // stdout/stderr were configured as pipes above, so `take()` must
    // succeed here — the Option arm exists only because of Command's
    // stdio API, not because pipe availability can vary at runtime.
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

    // `wait_with_timeout` only surfaces `Timeout` — see its doc comment
    // for why OS-level try_wait errors are treated as an invariant.
    let timeout = Duration::from_secs(timeout_secs);
    let status = match wait_with_timeout(|| child.try_wait(), std::thread::sleep, timeout) {
        Ok(s) => s,
        Err(WaitError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(format!("Timed out after {}s", timeout_secs));
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let code = status.code().unwrap_or(1);
    Ok((code, stdout, stderr))
}

/// Resolve mode from flags and state file.
///
/// Priority: --auto > --manual > state.skills.flow-complete > "auto".
pub fn resolve_mode(auto: bool, manual: bool, state: Option<&Value>) -> String {
    if auto {
        return "auto".to_string();
    }
    if manual {
        return "manual".to_string();
    }
    if let Some(st) = state {
        if let Some(skill_config) = st.get("skills").and_then(|s| s.get("flow-complete")) {
            if let Some(s) = skill_config.as_str() {
                return s.to_string();
            }
            if skill_config.is_object() {
                return skill_config
                    .get("continue")
                    .and_then(|v| v.as_str())
                    .unwrap_or("auto")
                    .to_string();
            }
        }
    }
    "auto".to_string()
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

/// Call phase-transition --action enter via the runner.
/// Returns parsed JSON value on success, error message on failure.
fn phase_transition_enter(
    branch: &str,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Result<Value, String> {
    let result = runner(
        &[
            bin_flow,
            "phase-transition",
            "--phase",
            "flow-complete",
            "--action",
            "enter",
            "--branch",
            branch,
        ],
        LOCAL_TIMEOUT,
    );
    match result {
        Err(e) => Err(e),
        Ok((code, stdout, stderr)) => {
            if code != 0 {
                return Err(stderr.trim().to_string());
            }
            serde_json::from_str(stdout.trim())
                .map_err(|_| format!("Invalid JSON from phase-transition: {}", stdout))
        }
    }
}

/// Check PR state via gh pr view. Returns PR state string on success.
pub fn check_pr_status(
    pr_number: Option<i64>,
    branch: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Result<String, String> {
    let identifier = if let Some(n) = pr_number {
        n.to_string()
    } else if !branch.is_empty() {
        branch.to_string()
    } else {
        return Err("No PR number or branch to check".to_string());
    };
    let result = runner(
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
    );
    match result {
        Err(e) => Err(e),
        Ok((code, stdout, stderr)) => {
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
    }
}

/// Merge origin/main into the current branch.
///
/// Returns one of:
///   ("clean", None) — already up to date
///   ("merged", None) — merged successfully (new commits)
///   ("conflict", Some(files_array)) — merge conflicts
///   ("error", Some(message_string)) — unexpected error
pub fn merge_main(runner: &dyn Fn(&[&str], u64) -> CmdResult) -> (String, Option<Value>) {
    // Fetch
    match runner(&["git", "fetch", "origin", "main"], NETWORK_TIMEOUT) {
        Err(e) => return ("error".to_string(), Some(json!(e))),
        Ok((code, _, stderr)) if code != 0 => {
            return ("error".to_string(), Some(json!(stderr.trim())));
        }
        Ok(_) => {}
    }

    // Check if already up to date
    match runner(
        &["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
        LOCAL_TIMEOUT,
    ) {
        Err(e) => return ("error".to_string(), Some(json!(e))),
        Ok((code, _, _)) => {
            if code == 0 {
                return ("clean".to_string(), None);
            }
        }
    }

    // Merge
    match runner(&["git", "merge", "origin/main"], NETWORK_TIMEOUT) {
        Err(e) => ("error".to_string(), Some(json!(e))),
        Ok((code, _, stderr)) => {
            if code == 0 {
                // Merged successfully — push
                match runner(&["git", "push"], NETWORK_TIMEOUT) {
                    Err(e) => (
                        "error".to_string(),
                        Some(json!(format!("Merge succeeded but push failed: {}", e))),
                    ),
                    Ok((push_code, _, push_stderr)) => {
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
                    }
                }
            } else {
                // Merge failed — check for conflicts
                match runner(&["git", "status", "--porcelain"], LOCAL_TIMEOUT) {
                    Err(_) => ("error".to_string(), Some(json!(stderr.trim()))),
                    Ok((_, status_stdout, _)) => {
                        let conflicts = parse_conflict_files(&status_stdout);
                        if !conflicts.is_empty() {
                            ("conflict".to_string(), Some(json!(conflicts)))
                        } else {
                            ("error".to_string(), Some(json!(stderr.trim())))
                        }
                    }
                }
            }
        }
    }
}

/// Core preflight logic with injectable runner.
pub fn preflight_inner(
    branch: Option<&str>,
    auto: bool,
    manual: bool,
    root: &Path,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
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

    // Read state file. External-input audit: `branch` may be the
    // `--branch` CLI override per `.claude/rules/external-input-validation.md`;
    // slash-containing or empty values cannot address flat
    // `.flow-states/` paths, so use `try_new` and surface a structured
    // error rather than panicking.
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

    // Resolve mode
    let mode = resolve_mode(auto, manual, state.as_ref());

    // Warnings
    let warnings = match state.as_ref() {
        Some(s) => check_learn_phase(s),
        None => Vec::new(),
    };

    // Phase transition enter (only if state file exists)
    if state.is_some() {
        if let Err(e) = phase_transition_enter(&branch, bin_flow, runner) {
            return json!({
                "status": "error",
                "message": format!("Phase transition failed: {}", e)
            });
        }

        // Set step counters
        let _ = mutate_state(&state_path, &mut |s| {
            if !(s.is_object() || s.is_null()) {
                return;
            }
            s["complete_steps_total"] = json!(COMPLETE_STEPS_TOTAL);
            s["complete_step"] = json!(1);
        });
    }

    // Check PR status
    let pr_number = state
        .as_ref()
        .and_then(|s| s.get("pr_number"))
        .and_then(|v| v.as_i64());
    let pr_state = match check_pr_status(pr_number, &branch, runner) {
        Ok(s) => s,
        Err(e) => {
            return json!({"status": "error", "message": e});
        }
    };

    // Build base result (order preserved via preserve_order feature)
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

    // Dispatch on PR state
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
            let (merge_status, merge_data) = merge_main(runner);
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

/// Production wrapper — resolves branch, root, bin_flow, runner automatically.
pub fn preflight(branch: Option<&str>, auto: bool, manual: bool, root: Option<&Path>) -> Value {
    let default_root = project_root();
    let root_ref: &Path = root.unwrap_or(&default_root);
    let resolved_branch: Option<String> = match branch {
        Some(b) => Some(b.to_string()),
        None => current_branch(),
    };
    preflight_inner(
        resolved_branch.as_deref(),
        auto,
        manual,
        root_ref,
        &bin_flow_path(),
        &run_cmd_with_timeout,
    )
}

/// Main-arm dispatch: returns (value, exit code).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let result = preflight(args.branch.as_deref(), args.auto, args.manual, None);
    let code = if result["status"] == "ok" { 0 } else { 1 };
    (result, code)
}
