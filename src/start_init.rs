//! Consolidated start-init: lock acquire + prime-check + upgrade-check +
//! prompt write + init-state + label-issues in a single command.
//!
//! Reduces the first ~8 tool calls of flow-start to 1. Returns JSON with
//! status "ready" (proceed to start-gate), "locked" (another start holds
//! the lock), or "error" (stop and report).
//!
//! Return type of `run_impl_main` is `(Value, i32)`: status-error JSON
//! goes through `Ok` with a `status: error` field; exit code `1` is
//! reserved for infrastructure failures (plugin root not found, etc.).
//!
//! Logic is driven entirely through the compiled binary; integration
//! tests use real git, controllable `gh` stubs, and `CLAUDE_PLUGIN_ROOT`
//! manipulation in a `TempDir` fixture.

use std::fs;
use std::path::Path;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::{acquire, queue_path, release};
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowStatesDir;
use crate::label_issues::{label_issues, LABEL};
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

/// Upgrade-check binder. Resolves the plugin.json path and runs the
/// real `upgrade_check_impl` against the GitHub CLI.
fn run_upgrade_check(plug_root: &Path) -> Value {
    let plugin_json = plug_root.join(".claude-plugin").join("plugin.json");
    let mut gh_cmd = |owner_repo: &str, timeout_secs: u64| -> GhResult {
        upgrade_check::run_gh_cmd(owner_repo, timeout_secs)
    };
    upgrade_check::upgrade_check_impl(&plugin_json, 10, &mut gh_cmd)
}

/// Core start-init logic. Returns `Result<Value, String>` where Err is
/// reserved for infrastructure failures that surface as exit 1.
fn run_impl(args: &Args, root: &Path, cwd: &Path) -> Result<Value, String> {
    let queue_dir = queue_path(root);
    // The `.flow-states/` directory is shared across every branch on
    // this machine; FlowStatesDir addresses it without a branch scope.
    let state_dir = FlowStatesDir::new(root).path().to_path_buf();
    let _ = fs::create_dir_all(&state_dir);

    let plug_root = match plugin_root() {
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

    // Step 2: Prime check. Err surfaces infrastructure failures
    // (malformed plugin.json, unreadable plugin.json) as a business
    // error released under the start lock.
    let prime_result = prime_check::run_impl(cwd, &plug_root)
        .unwrap_or_else(|e| json!({"status": "error", "message": e}));

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
    let auto_upgraded = prime_result["auto_upgraded"] == json!(true);

    // Step 3: Upgrade check (best-effort, never errors)
    let upgrade_result = run_upgrade_check(&plug_root);
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
    // cwd and root are both always real paths at this point — cwd is
    // the current working directory of a running flow-rs process and
    // root resolved from project_root(). canonicalize Err on either is
    // a programmer-visible panic.
    let cwd_canon = cwd.canonicalize().expect("cwd must canonicalize");
    let root_canon = root.canonicalize().expect("root must canonicalize");
    let relative_cwd = cwd_canon
        .strip_prefix(&root_canon)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_default();

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

    // Spawn `init-state` via our own binary. current_exe() fails only
    // when the binary has been unlinked mid-run — treated as a
    // programmer-visible panic. Command::output() fails only on spawn
    // failure; after flow-rs is already running, respawning itself is
    // reliable, so `.expect()` applies per
    // `.claude/rules/testability-means-simplicity.md`.
    let self_exe = std::env::current_exe().expect("current executable path is resolvable");
    let init_output = Command::new(&self_exe)
        .args(&cmd_args)
        .current_dir(cwd)
        .output()
        .expect("init-state subprocess spawn");

    // Prompt file cleanup is handled by init-state's read_prompt_file()
    // which reads and deletes the file atomically.

    // init-state is our own binary; its stdout is always the canonical
    // JSON contract. An empty or unparseable response is a programmer-
    // visible panic per `.claude/rules/testability-means-simplicity.md`.
    let init_stdout = String::from_utf8_lossy(&init_output.stdout);
    let init_line = init_stdout
        .trim()
        .lines()
        .last()
        .expect("init-state stdout must contain at least one JSON line");
    let init_json: Value =
        serde_json::from_str(init_line).expect("init-state stdout must parse as JSON");

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
        let result = label_issues(&issue_numbers, "add");
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
        // prime_check always sets old_version/new_version alongside
        // auto_upgraded=true; copy both unconditionally.
        response["old_version"] = prime_result["old_version"].clone();
        response["new_version"] = prime_result["new_version"].clone();
    }

    if upgrade_result["status"] != "current" && upgrade_result["status"] != "unknown" {
        response["upgrade"] = upgrade_result;
    }

    if !issue_numbers.is_empty() {
        response["labels"] = labels_result;
    }

    Ok(response)
}

/// Production main-arm entry point. Infrastructure errors (plugin root
/// undetectable) surface as `(err_json, 1)`; every other scenario
/// returns `(ok_value, 0)`. Takes `root: &Path` and `cwd: &Path` per
/// `.claude/rules/rust-patterns.md` "Main-arm dispatch" so inline tests
/// can pass a `TempDir` fixture instead of the host
/// `project_root()`/`current_dir()`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    match run_impl(args, root, cwd) {
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
