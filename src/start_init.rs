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
//! upgrade-check, and the init-state subprocess runner). Inline tests
//! drive the plugin-root-None and init-state-dispatch error branches
//! with stub closures against a `TempDir` fixture, so those paths are
//! testable without spawning the real `init-state` binary, touching
//! `CLAUDE_PLUGIN_ROOT`, or making a GitHub API call. Production
//! [`run_impl`] is a one-line binder.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Output};

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::{acquire, queue_path, release};
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowStatesDir;
use crate::git::project_root;
use crate::label_issues::{label_issues, LABEL};
use crate::output::json_error;
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
fn default_init_state_runner(args: &[String], cwd: &Path) -> Result<Output, String> {
    let self_exe = std::env::current_exe()
        .map_err(|e| format!("Could not determine current executable: {}", e))?;
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

    let plug_root = plug_root_finder()
        .ok_or_else(|| "CLAUDE_PLUGIN_ROOT not set and could not detect plugin root".to_string())?;

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

    // Capture version info for response
    let auto_upgraded = prime_result
        .get("auto_upgraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut version_info = json!({});
    if auto_upgraded {
        version_info["auto_upgraded"] = json!(true);
        if let Some(old) = prime_result.get("old_version") {
            version_info["old_version"] = old.clone();
        }
        if let Some(new) = prime_result.get("new_version") {
            version_info["new_version"] = new.clone();
        }
    }

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
        let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
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
        if let Some(old) = version_info.get("old_version") {
            response["old_version"] = old.clone();
        }
        if let Some(new) = version_info.get("new_version") {
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

/// Production entry point: binds [`run_impl_with_deps`] to the real
/// [`plugin_root`], [`prime_check::run_impl`], the default upgrade
/// check, and the default init-state subprocess runner.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_impl_with_deps(
        args,
        &root,
        &cwd,
        &plugin_root,
        &prime_check::run_impl,
        &default_upgrade_check,
        &default_init_state_runner,
    )
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    // --- run_impl_with_deps ---

    /// Build a fake `Output` with the given stdout bytes and exit code 0.
    fn fake_output(stdout: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn ok_prime_check(_cwd: &Path, _plug_root: &Path) -> Result<Value, String> {
        Ok(json!({"status": "ok"}))
    }

    fn ok_upgrade_check(_plug_root: &Path) -> Value {
        json!({"status": "current"})
    }

    fn panic_init_runner(_args: &[String], _cwd: &Path) -> Result<Output, String> {
        panic!("init_state_runner must not be called on plugin-root error path");
    }

    #[test]
    fn start_init_plugin_root_none_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let args = Args {
            feature_name: "plugroot-none".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = || -> Option<PathBuf> { None };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &panic_init_runner,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("CLAUDE_PLUGIN_ROOT"));
    }

    #[test]
    fn start_init_init_state_spawn_failure_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "spawn-fail".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> {
            Err("Failed to spawn init-state: no such file".to_string())
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &runner,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to spawn init-state"));
    }

    #[test]
    fn start_init_init_state_parse_fallback() {
        // Runner returns Output with empty stdout → fallback JSON fires.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "parse-fallback".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> { Ok(fake_output("")) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &runner,
        )
        .unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(
            result["message"].as_str().unwrap(),
            "Could not parse init-state output"
        );
        assert_eq!(result["step"], "init_state");

        // Lock must be released — the release_and_error helper deletes
        // the queue entry.
        let queue_entry = root.join(".flow-states/start-queue/parse-fallback");
        assert!(
            !queue_entry.exists(),
            "lock must be released on parse fallback error"
        );
    }

    #[test]
    fn start_init_init_state_error_releases_lock_via_seam() {
        // Runner returns Output with a valid error JSON → release lock.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "init-err".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> {
            Ok(fake_output(
                r#"{"status": "error", "message": "init-state refused", "step": "seeded_error"}"#,
            ))
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &runner,
        )
        .unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(result["message"], "init-state refused");
        assert_eq!(result["step"], "seeded_error");

        let queue_entry = root.join(".flow-states/start-queue/init-err");
        assert!(
            !queue_entry.exists(),
            "lock must be released on init-state error"
        );
    }

    #[test]
    fn start_init_prime_check_error_releases_lock_via_seam() {
        // Inject a prime_check that returns Err → lock release path fires.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "prime-err".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let err_prime = |_: &Path, _: &Path| -> Result<Value, String> {
            Err("missing plugin.json".to_string())
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &err_prime,
            &ok_upgrade_check,
            &panic_init_runner,
        )
        .unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "prime_check");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("missing plugin.json"));
    }

    #[test]
    fn start_init_happy_path_via_seam_returns_ready() {
        // Sanity: the full happy path via stubbed runners returns
        // status=ready with the expected branch derivation.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "happy-seam".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> {
            Ok(fake_output(r#"{"status": "ok", "branch": "happy-seam"}"#))
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &runner,
        )
        .unwrap();
        assert_eq!(result["status"], "ready");
        assert_eq!(result["branch"], "happy-seam");
    }

    #[test]
    fn start_init_auto_upgraded_propagates_to_response() {
        // prime_check returns auto_upgraded:true with old/new versions →
        // response carries both fields.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "auto-up".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let upgraded_prime = |_: &Path, _: &Path| -> Result<Value, String> {
            Ok(json!({
                "status": "ok",
                "auto_upgraded": true,
                "old_version": "1.0.0",
                "new_version": "1.0.1",
            }))
        };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> {
            Ok(fake_output(r#"{"status": "ok"}"#))
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &upgraded_prime,
            &ok_upgrade_check,
            &runner,
        )
        .unwrap();
        assert_eq!(result["status"], "ready");
        assert_eq!(result["auto_upgraded"], true);
        assert_eq!(result["old_version"], "1.0.0");
        assert_eq!(result["new_version"], "1.0.1");
    }

    #[test]
    fn start_init_upgrade_available_adds_upgrade_field() {
        // upgrade_check returns status=upgrade_available → response
        // includes the upgrade field.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        let args = Args {
            feature_name: "upgrade-avail".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
        let upgrade = |_: &Path| -> Value {
            json!({"status": "upgrade_available", "latest": "99.0.0", "installed": "1.0.0"})
        };
        let runner = |_: &[String], _: &Path| -> Result<Output, String> {
            Ok(fake_output(r#"{"status": "ok"}"#))
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &upgrade,
            &runner,
        )
        .unwrap();
        assert_eq!(result["status"], "ready");
        assert_eq!(result["upgrade"]["status"], "upgrade_available");
        assert_eq!(result["upgrade"]["latest"], "99.0.0");
    }

    #[test]
    fn start_init_lock_already_held_returns_locked() {
        // Pre-create a queue entry for another feature so acquire
        // returns "locked". Exercises the early-return branch.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let plug_root = root.clone();
        // Seed another feature's lock entry
        let queue_dir = root.join(".flow-states/start-queue");
        fs::create_dir_all(&queue_dir).unwrap();
        fs::write(queue_dir.join("other-feature"), "").unwrap();

        let args = Args {
            feature_name: "blocked-feature".to_string(),
            auto: false,
            prompt_file: None,
        };
        let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &finder,
            &ok_prime_check,
            &ok_upgrade_check,
            &panic_init_runner,
        )
        .unwrap();
        assert_eq!(result["status"], "locked");
        assert_eq!(result["feature"], "other-feature");
    }
}
