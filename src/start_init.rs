//! Consolidated start-init: lock acquire + prime-check + upgrade-check +
//! prompt write + init-state + label-issues in a single command.
//!
//! Reduces the first ~8 tool calls of flow-start to 1. Returns JSON with
//! status "ready" (proceed to start-gate), "locked" (another start holds
//! the lock), or "error" (stop and report).
//!
//! Return type is `Result<Value, String>`: status-error JSON goes through
//! `Ok` with a `status: error` field. `Err(String)` is reserved for
//! infrastructure failures (plugin root not found, etc.) that should exit 1.
//!
//! # Dependency-injected core
//!
//! [`run_impl_with_deps`] is the fully-testable core: it accepts the
//! project root, cwd, and four subprocess/environment callouts as
//! injectable closures (plugin-root detection, prime-check,
//! upgrade-check, and the init-state subprocess runner). Integration
//! tests drive the plugin-root-None and init-state-dispatch error
//! branches with stub closures against a `TempDir` fixture. Production
//! [`run_impl`] is a one-line binder.
//!
//! ## Why start_init has `run_impl_main_with_deps`
//!
//! Among the four start-family modules, only `start_init` exposes
//! [`run_impl_main_with_deps`] alongside [`run_impl_main`]. The
//! asymmetry reflects a concrete testability need: `start_init` is
//! the one module whose `run_impl` can return `Result::Err` at the
//! Rust level (when `plug_root_finder` yields `None` or the
//! init-state subprocess fails to spawn). The `Err` arm of
//! `run_impl_main` maps to exit code `1` per the `(err_json, 1)`
//! dispatch convention, and the only way to exercise that arm from a
//! unit test is to inject a dep that produces `Err`. Hence the
//! seam-accepting entry point. `start_gate`, `start_workspace`, and
//! `start_finalize` have no reachable `Err` path in `run_impl`, so
//! their `run_impl_main` is a trivial `(v, 0)` wrapper with no seam
//! variant.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::{acquire, queue_path, release};
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowStatesDir;
use crate::label_issues::{default_timeout, gh_child_factory, label_issues_with_runner, LABEL};
use crate::prime_check;
use crate::upgrade_check::{self, GhResult};
use crate::utils::{
    branch_name, check_duplicate_issue, extract_issue_numbers, fetch_issue_info, plugin_root,
};

#[derive(Parser, Debug)]
#[command(name = "start-init", about = "Consolidated start initialization")]
pub struct Args {
    /// Feature name (sanitized form for lock queue entry)
    pub feature_name: String,

    /// Override all skills to fully autonomous preset
    #[arg(long)]
    pub auto: bool,

    /// Path to file containing start prompt
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,
}

/// Default subprocess runner for `init-state`. Spawns the current
/// executable with the given args and cwd, capturing stdout/stderr.
/// Used as the production closure reference inside this module;
/// integration tests drive its error paths through `run_impl_main`
/// with fixtures that force the subprocess to fail.
fn default_init_state_runner(args: &[String], cwd: &Path) -> Result<Output, String> {
    // std::env::current_exe fails only on platforms without a /proc or
    // equivalent, or when the binary has been unlinked mid-run. On
    // every supported target the call succeeds — a failure here is a
    // programmer-visible panic.
    let self_exe = std::env::current_exe().expect("current executable path is resolvable");
    std::process::Command::new(&self_exe)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("Failed to spawn init-state: {}", e))
}

/// Default upgrade-check binder. Resolves the plugin.json path and runs
/// the real `upgrade_check_impl` against the GitHub CLI.
fn default_upgrade_check(plug_root: &Path) -> Value {
    let plugin_json = plug_root.join(".claude-plugin").join("plugin.json");
    let mut gh_cmd = |owner_repo: &str, timeout_secs: u64| -> GhResult {
        upgrade_check::run_gh_cmd(owner_repo, timeout_secs)
    };
    upgrade_check::upgrade_check_impl(&plugin_json, 10, &mut gh_cmd)
}

/// Testable core with injected project root, cwd, and the four
/// subprocess/environment callouts. Production [`run_impl`] binds
/// the closures to [`plugin_root`], [`prime_check::run_impl`],
/// [`default_upgrade_check`], and [`default_init_state_runner`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_impl_with_deps(
    args: &Args,
    root: &Path,
    cwd: &Path,
    plug_root_finder: &dyn Fn() -> Option<PathBuf>,
    prime_check_fn: &dyn Fn(&Path, &Path) -> Result<Value, String>,
    upgrade_check_fn: &dyn Fn(&Path) -> Value,
    init_state_runner: &dyn Fn(&[String], &Path) -> Result<Output, String>,
) -> Result<Value, String> {
    let queue_dir = queue_path(root);
    // The `.flow-states/` directory is shared across every branch on
    // this machine; FlowStatesDir addresses it without a branch scope.
    let state_dir = FlowStatesDir::new(root).path().to_path_buf();
    let _ = fs::create_dir_all(&state_dir);

    let plug_root = match plug_root_finder() {
        Some(p) => p,
        None => {
            return Err("CLAUDE_PLUGIN_ROOT not set and could not detect plugin root".to_string());
        }
    };

    // --- Pre-lock: derive canonical branch name ---
    // Read prompt non-destructively (init-state will read+delete via --prompt-file later)
    let prompt_text = args
        .prompt_file
        .as_ref()
        .and_then(|pf| fs::read_to_string(pf).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| args.feature_name.clone());

    let issue_numbers = extract_issue_numbers(&prompt_text);
    let branch = if !issue_numbers.is_empty() {
        match fetch_issue_info(issue_numbers[0]) {
            Some(info) => {
                // Flow In-Progress label guard (cross-machine WIP detection)
                if info.labels.iter().any(|l| l == LABEL) {
                    return Ok(json!({
                        "status": "error",
                        "message": format!(
                            "Issue #{} already carries the '{}' label — another flow is in progress. Resume the existing flow in its worktree, or reference a different issue.",
                            issue_numbers[0], LABEL
                        ),
                        "step": "flow_in_progress_label",
                    }));
                }
                branch_name(&info.title)
            }
            None => {
                return Ok(json!({
                    "status": "error",
                    "message": format!("Could not fetch title for issue #{}", issue_numbers[0]),
                    "step": "fetch_issue_title",
                }));
            }
        }
    } else {
        branch_name(&args.feature_name)
    };

    // Duplicate issue guard (before lock — no lock to leak)
    if !issue_numbers.is_empty() {
        if let Some(dup) = check_duplicate_issue(root, &issue_numbers, &branch) {
            return Ok(json!({
                "status": "error",
                "message": format!(
                    "Issue already has an active flow on branch '{}' (phase: {}, PR: {}). Resume the existing flow instead.",
                    dup.branch, dup.phase, dup.pr_url
                ),
                "step": "duplicate_issue",
            }));
        }
    }

    // Step 1: Acquire lock (on canonical branch name)
    let lock_result = acquire(&branch, &queue_dir);
    let _ = append_log(
        root,
        &branch,
        &format!(
            "[Phase 1] start-init — lock acquire ({})",
            lock_result["status"]
        ),
    );

    if lock_result["status"] == "locked" {
        return Ok(json!({
            "status": "locked",
            "feature": lock_result["feature"],
            "lock_path": lock_result["lock_path"],
        }));
    }

    // Helper: release lock on error and return error JSON
    let release_and_error = |msg: &str, step: &str| -> Value {
        release(&branch, &queue_dir);
        json!({
            "status": "error",
            "message": msg,
            "step": step,
        })
    };

    // Step 2: Prime check
    let prime_result = match prime_check_fn(cwd, &plug_root) {
        Ok(v) => v,
        Err(e) => {
            let _ = append_log(
                root,
                &branch,
                &format!(
                    "[Phase 1] start-init — prime-check infrastructure error: {}",
                    e
                ),
            );
            return Ok(release_and_error(&e, "prime_check"));
        }
    };

    let _ = append_log(
        root,
        &branch,
        &format!(
            "[Phase 1] start-init — prime-check ({})",
            prime_result["status"]
        ),
    );

    if prime_result["status"] == "error" {
        let msg = prime_result["message"]
            .as_str()
            .unwrap_or("Prime check failed")
            .to_string();
        return Ok(release_and_error(&msg, "prime_check"));
    }

    // Capture auto_upgraded state for the final response assembly.
    let auto_upgraded = prime_result
        .get("auto_upgraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Step 3: Upgrade check (best-effort, never errors)
    let upgrade_result = upgrade_check_fn(&plug_root);
    let _ = append_log(
        root,
        &branch,
        &format!(
            "[Phase 1] start-init — upgrade-check ({})",
            upgrade_result["status"]
        ),
    );

    // Compute relative_cwd: where inside the project root the user
    // started the flow. Empty string means project root (the common
    // case). When the user runs `/flow:flow-start` from a subdirectory
    // of a mono-repo (e.g. `api/`), this captures `api` so the agent
    // lands back in the same subdirectory after the worktree is created.
    // canonicalize() handles symlinks; strip_prefix returns relative.
    let relative_cwd = {
        let cwd_canon = match cwd.canonicalize() {
            Ok(p) => p,
            Err(_) => cwd.to_path_buf(),
        };
        let root_canon = match root.canonicalize() {
            Ok(p) => p,
            Err(_) => root.to_path_buf(),
        };
        match cwd_canon.strip_prefix(&root_canon) {
            Ok(rel) => rel.to_string_lossy().into_owned(),
            Err(_) => String::new(),
        }
    };

    // Step 4: Call init-state via injected runner
    let mut cmd_args = vec![
        "init-state".to_string(),
        args.feature_name.clone(),
        "--branch".to_string(),
        branch.clone(),
        "--start-step".to_string(),
        "1".to_string(),
        "--start-steps-total".to_string(),
        "5".to_string(),
        "--relative-cwd".to_string(),
        relative_cwd.clone(),
    ];
    if let Some(ref pf) = args.prompt_file {
        cmd_args.push("--prompt-file".to_string());
        cmd_args.push(pf.clone());
    }
    if args.auto {
        cmd_args.push("--auto".to_string());
    }

    let init_output = init_state_runner(&cmd_args, cwd)?;

    // Prompt file cleanup is handled by init-state's read_prompt_file()
    // which reads and deletes the file atomically.

    let init_stdout = String::from_utf8_lossy(&init_output.stdout);
    let init_json: Value = init_stdout
        .trim()
        .lines()
        .last()
        .and_then(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(
            || json!({"status": "error", "message": "Could not parse init-state output"}),
        );

    let _ = append_log(
        root,
        &branch,
        &format!(
            "[Phase 1] start-init — init-state ({})",
            init_json["status"]
        ),
    );

    if init_json["status"] == "error" {
        let msg = init_json["message"]
            .as_str()
            .unwrap_or("init-state failed")
            .to_string();
        let step = init_json["step"]
            .as_str()
            .unwrap_or("init_state")
            .to_string();
        return Ok(release_and_error(&msg, &step));
    }

    // Update step counter for TUI (step 1 = init)
    let state_path = state_dir.join(format!("{}.json", branch));
    update_step(&state_path, 1);

    // Step 5: Label issues (best-effort)
    // issue_numbers already derived in the pre-lock section
    let mut labels_result = json!({});
    if !issue_numbers.is_empty() {
        let result =
            label_issues_with_runner(&issue_numbers, "add", default_timeout(), &gh_child_factory);
        labels_result = json!({
            "labeled": result.labeled,
            "failed": result.failed,
        });
        let _ = append_log(
            root,
            &branch,
            &format!(
                "[Phase 1] start-init — label-issues (labeled: {:?}, failed: {:?})",
                result.labeled, result.failed
            ),
        );
    }

    // Build response
    let mut response = json!({
        "status": "ready",
        "branch": branch,
        "state_file": format!(".flow-states/{}.json", branch),
    });

    if auto_upgraded {
        response["auto_upgraded"] = json!(true);
        if let Some(old) = prime_result.get("old_version") {
            response["old_version"] = old.clone();
        }
        if let Some(new) = prime_result.get("new_version") {
            response["new_version"] = new.clone();
        }
    }

    if upgrade_result["status"] != "current" && upgrade_result["status"] != "unknown" {
        response["upgrade"] = upgrade_result;
    }

    if !issue_numbers.is_empty() {
        response["labels"] = labels_result;
    }

    Ok(response)
}

/// Testable main-arm entry point with injected dependencies.
///
/// Wraps [`run_impl_with_deps`] into the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. `run_impl_with_deps` returns
/// `Err` when `plug_root_finder` yields `None` or `init_state_runner`
/// fails — both infrastructure failures that surface as
/// `(err_json, 1)`. Every other scenario returns `(Ok value, 0)`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_impl_main_with_deps(
    args: &Args,
    root: &Path,
    cwd: &Path,
    plug_root_finder: &dyn Fn() -> Option<PathBuf>,
    prime_check_fn: &dyn Fn(&Path, &Path) -> Result<Value, String>,
    upgrade_check_fn: &dyn Fn(&Path) -> Value,
    init_state_runner: &dyn Fn(&[String], &Path) -> Result<Output, String>,
) -> (Value, i32) {
    match run_impl_with_deps(
        args,
        root,
        cwd,
        plug_root_finder,
        prime_check_fn,
        upgrade_check_fn,
        init_state_runner,
    ) {
        Ok(v) => (v, 0),
        Err(e) => (
            json!({
                "status": "error",
                "message": e,
                "step": "start_init_run_impl",
            }),
            1,
        ),
    }
}

/// Production main-arm entry point: binds [`run_impl_main_with_deps`]
/// to the real `plugin_root`, `prime_check::run_impl`, default
/// upgrade check, and default init-state subprocess runner. Takes
/// `root: &Path` and `cwd: &Path` per `.claude/rules/rust-patterns.md`
/// "Main-arm dispatch" so inline tests can pass a `TempDir` fixture
/// instead of the host `project_root()`/`current_dir()`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    run_impl_main_with_deps(
        args,
        root,
        cwd,
        &plugin_root,
        &prime_check::run_impl,
        &default_upgrade_check,
        &default_init_state_runner,
    )
}
