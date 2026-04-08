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

use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
use crate::utils::{bin_flow_path, derive_worktree, parse_conflict_files};

const LOCAL_TIMEOUT: u64 = 30;
const NETWORK_TIMEOUT: u64 = 60;
/// Legacy step count — retained for backward compatibility when
/// complete-preflight is called as a standalone subcommand.
/// complete_fast.rs uses COMPLETE_STEPS_TOTAL=5 (the new reduced count).
const COMPLETE_STEPS_TOTAL: i64 = 7;

pub type CmdResult = Result<(i32, String, String), String>;

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
/// never observe the child exiting. See `.claude/rules/rust-port-parity.md`
/// "Subprocess Timeout Parity".
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

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_handle {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr_handle {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
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
            Err(e) => {
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(e.to_string());
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

    // Read state file
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
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
        let _ = mutate_state(&state_path, |s| {
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

/// CLI entry point.
pub fn run(args: Args) {
    let result = preflight(args.branch.as_deref(), args.auto, args.manual, None);
    println!("{}", result);
    if result["status"] != "ok" {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::rc::Rc;

    const PT_ENTER_OK: &str = r#"{"status": "ok", "phase": "flow-complete", "action": "enter", "visit_count": 1, "first_visit": true}"#;

    fn mock_runner(responses: Vec<CmdResult>) -> impl Fn(&[&str], u64) -> CmdResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |_args: &[&str], _timeout: u64| -> CmdResult {
            queue
                .borrow_mut()
                .pop_front()
                .expect("mock_runner: no more responses")
        }
    }

    fn tracking_runner(
        responses: Vec<CmdResult>,
        calls: Rc<RefCell<Vec<Vec<String>>>>,
    ) -> impl Fn(&[&str], u64) -> CmdResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |args: &[&str], _timeout: u64| -> CmdResult {
            calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            queue
                .borrow_mut()
                .pop_front()
                .expect("tracking_runner: no more responses")
        }
    }

    fn ok(stdout: &str) -> CmdResult {
        Ok((0, stdout.to_string(), String::new()))
    }

    fn ok_empty() -> CmdResult {
        Ok((0, String::new(), String::new()))
    }

    fn fail(stderr: &str) -> CmdResult {
        Ok((1, String::new(), stderr.to_string()))
    }

    fn err(msg: &str) -> CmdResult {
        Err(msg.to_string())
    }

    fn setup_state(root: &Path, branch: &str, learn_status: &str, skills: Option<Value>) {
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let mut state = json!({
            "schema_version": 1,
            "branch": branch,
            "repo": "test/test",
            "pr_number": 42,
            "pr_url": "https://github.com/test/test/pull/42",
            "prompt": "work on issue #100",
            "phases": {
                "flow-start": {"status": "complete"},
                "flow-plan": {"status": "complete"},
                "flow-code": {"status": "complete"},
                "flow-code-review": {"status": "complete"},
                "flow-learn": {"status": learn_status},
                "flow-complete": {"status": "pending"}
            }
        });
        if let Some(s) = skills {
            state["skills"] = s;
        }
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();
    }

    // --- resolve_mode ---

    #[test]
    fn resolve_mode_auto_flag_wins() {
        let state = json!({"skills": {"flow-complete": "manual"}});
        assert_eq!(resolve_mode(true, false, Some(&state)), "auto");
    }

    #[test]
    fn resolve_mode_manual_flag_wins_over_state() {
        let state = json!({"skills": {"flow-complete": "auto"}});
        assert_eq!(resolve_mode(false, true, Some(&state)), "manual");
    }

    #[test]
    fn resolve_mode_state_string() {
        let state = json!({"skills": {"flow-complete": "manual"}});
        assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
    }

    #[test]
    fn resolve_mode_state_dict_continue() {
        let state = json!({"skills": {"flow-complete": {"continue": "manual", "commit": "auto"}}});
        assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
    }

    #[test]
    fn resolve_mode_state_dict_no_continue_defaults_auto() {
        let state = json!({"skills": {"flow-complete": {"commit": "auto"}}});
        assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
    }

    #[test]
    fn resolve_mode_no_state_defaults_auto() {
        assert_eq!(resolve_mode(false, false, None), "auto");
    }

    #[test]
    fn resolve_mode_state_without_skills_defaults_auto() {
        let state = json!({"branch": "test"});
        assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
    }

    // --- check_learn_phase ---

    #[test]
    fn check_learn_phase_complete_no_warning() {
        let state = json!({"phases": {"flow-learn": {"status": "complete"}}});
        assert!(check_learn_phase(&state).is_empty());
    }

    #[test]
    fn check_learn_phase_pending_emits_warning() {
        let state = json!({"phases": {"flow-learn": {"status": "pending"}}});
        let warnings = check_learn_phase(&state);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Phase 5"));
        assert!(warnings[0].contains("pending"));
    }

    #[test]
    fn check_learn_phase_missing_treated_as_pending() {
        let state = json!({"phases": {}});
        let warnings = check_learn_phase(&state);
        assert_eq!(warnings.len(), 1);
    }

    // --- check_pr_status ---

    #[test]
    fn check_pr_status_no_identifier_returns_error() {
        let runner = mock_runner(vec![]);
        let result = check_pr_status(None, "", &runner);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_lowercase().contains("no pr number"));
    }

    #[test]
    fn check_pr_status_uses_pr_number_when_provided() {
        let runner = mock_runner(vec![ok("OPEN")]);
        let result = check_pr_status(Some(42), "some-branch", &runner);
        assert_eq!(result.unwrap(), "OPEN");
    }

    #[test]
    fn check_pr_status_falls_back_to_branch() {
        let runner = mock_runner(vec![ok("MERGED")]);
        let result = check_pr_status(None, "feature-xyz", &runner);
        assert_eq!(result.unwrap(), "MERGED");
    }

    #[test]
    fn check_pr_status_gh_failure_returns_error() {
        let runner = mock_runner(vec![fail("Could not resolve to a Pull Request")]);
        let result = check_pr_status(Some(42), "b", &runner);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not resolve"));
    }

    #[test]
    fn check_pr_status_gh_failure_empty_stderr_returns_generic_error() {
        let runner = mock_runner(vec![fail("")]);
        let result = check_pr_status(Some(42), "b", &runner);
        assert_eq!(result.unwrap_err(), "Could not find PR");
    }

    // --- merge_main ---

    #[test]
    fn merge_main_already_up_to_date() {
        let runner = mock_runner(vec![
            ok_empty(), // git fetch
            ok_empty(), // merge-base is ancestor
        ]);
        let (status, data) = merge_main(&runner);
        assert_eq!(status, "clean");
        assert!(data.is_none());
    }

    #[test]
    fn merge_main_new_commits_merged_and_pushed() {
        let runner = mock_runner(vec![
            ok_empty(),       // git fetch
            fail(""),         // merge-base not ancestor
            ok("Merge made"), // git merge
            ok_empty(),       // git push
        ]);
        let (status, data) = merge_main(&runner);
        assert_eq!(status, "merged");
        assert!(data.is_none());
    }

    #[test]
    fn merge_main_conflicts_detected() {
        let runner = mock_runner(vec![
            ok_empty(),                           // git fetch
            fail(""),                             // merge-base not ancestor
            fail("CONFLICT (content)"),           // git merge
            ok("UU lib/foo.py\nAA lib/bar.py\n"), // git status
        ]);
        let (status, data) = merge_main(&runner);
        assert_eq!(status, "conflict");
        let files: Vec<String> = data
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(files.contains(&"lib/foo.py".to_string()));
        assert!(files.contains(&"lib/bar.py".to_string()));
    }

    #[test]
    fn merge_main_fetch_error() {
        let runner = mock_runner(vec![fail("Could not resolve host")]);
        let (status, data) = merge_main(&runner);
        assert_eq!(status, "error");
        assert!(data
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Could not resolve"));
    }

    #[test]
    fn merge_main_merge_error_non_conflict() {
        let runner = mock_runner(vec![
            ok_empty(),      // fetch
            fail(""),        // merge-base not ancestor
            fail("generic"), // merge failed
            ok_empty(),      // status porcelain clean
        ]);
        let (status, _data) = merge_main(&runner);
        assert_eq!(status, "error");
    }

    #[test]
    fn merge_main_push_failure_after_merge() {
        let runner = mock_runner(vec![
            ok_empty(),              // fetch
            fail(""),                // merge-base not ancestor
            ok("Merge made"),        // merge ok
            fail("remote rejected"), // push fails
        ]);
        let (status, data) = merge_main(&runner);
        assert_eq!(status, "error");
        let msg = data.unwrap();
        let msg_str = msg.as_str().unwrap();
        assert!(msg_str.to_lowercase().contains("push failed"));
    }

    #[test]
    fn merge_main_timeout_returns_error() {
        let runner = mock_runner(vec![err("Timed out after 60s")]);
        let (status, _data) = merge_main(&runner);
        assert_eq!(status, "error");
    }

    // --- preflight_inner: happy paths ---

    #[test]
    fn preflight_happy_path_open_pr_clean_merge() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK), // phase-transition enter
            ok("OPEN"),      // gh pr view
            ok_empty(),      // git fetch
            ok_empty(),      // merge-base (already up to date)
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["pr_state"], "OPEN");
        assert_eq!(result["merge"], "clean");
        assert_eq!(result["mode"], "auto");
        assert_eq!(result["warnings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn preflight_pr_already_merged_returns_early() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("MERGED")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["pr_state"], "MERGED");
        assert!(result.get("merge").is_none());
    }

    #[test]
    fn preflight_pr_closed_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("CLOSED")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("closed"));
    }

    #[test]
    fn preflight_merge_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK),
            ok("OPEN"),
            ok_empty(),                           // fetch
            fail(""),                             // merge-base not ancestor
            fail("CONFLICT (content)"),           // merge
            ok("UU lib/foo.py\nAA lib/bar.py\n"), // status
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "conflict");
        let files: Vec<String> = result["conflict_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(files.contains(&"lib/foo.py".to_string()));
        assert!(files.contains(&"lib/bar.py".to_string()));
    }

    #[test]
    fn preflight_no_state_file_infers_from_git() {
        let dir = tempfile::tempdir().unwrap();
        // Do not create a state file
        fs::create_dir_all(dir.path().join(".flow-states")).unwrap();

        let runner = mock_runner(vec![
            // No phase-transition call because state missing
            ok("OPEN"), // gh pr view (by branch since no pr_number)
            ok_empty(), // fetch
            ok_empty(), // merge-base
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["inferred"], true);
    }

    // --- mode flags ---

    #[test]
    fn preflight_auto_flag_overrides_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(
            dir.path(),
            "test-feature",
            "complete",
            Some(json!({"flow-complete": "manual"})),
        );

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            true,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["mode"], "auto");
    }

    #[test]
    fn preflight_manual_flag_overrides_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(
            dir.path(),
            "test-feature",
            "complete",
            Some(json!({"flow-complete": "auto"})),
        );

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            true,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["mode"], "manual");
    }

    #[test]
    fn preflight_mode_from_state_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(
            dir.path(),
            "test-feature",
            "complete",
            Some(json!({"flow-complete": "manual"})),
        );

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["mode"], "manual");
    }

    // --- learn phase warning ---

    #[test]
    fn preflight_learn_pending_emits_warning() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "pending", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        let warnings = result["warnings"].as_array().unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings[0]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("phase 5"));
    }

    // --- step counter persistence ---

    #[test]
    fn preflight_sets_complete_step_counters_in_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let state_content =
            fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap();
        let state: Value = serde_json::from_str(&state_content).unwrap();
        assert_eq!(state["complete_steps_total"], json!(7));
        assert_eq!(state["complete_step"], json!(1));
    }

    // --- merged with new commits pushes ---

    #[test]
    fn preflight_merge_with_new_commits_pushes() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
        let runner = tracking_runner(
            vec![
                ok(PT_ENTER_OK),
                ok("OPEN"),
                ok_empty(),       // fetch
                fail(""),         // merge-base not ancestor
                ok("Merge made"), // merge
                ok_empty(),       // push
            ],
            calls.clone(),
        );

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["merge"], "merged");
        let push_calls: Vec<_> = calls
            .borrow()
            .iter()
            .filter(|c| c.iter().any(|a| a == "push"))
            .cloned()
            .collect();
        assert!(!push_calls.is_empty());
    }

    // --- phase transition invocation ---

    #[test]
    fn preflight_phase_transition_enter_called_with_correct_args() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
        let runner = tracking_runner(
            vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()],
            calls.clone(),
        );

        preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let pt_call = calls
            .borrow()
            .iter()
            .find(|c| c.iter().any(|a| a == "phase-transition"))
            .cloned()
            .expect("phase-transition call not found");
        assert!(pt_call.contains(&"--action".to_string()));
        assert!(pt_call.contains(&"enter".to_string()));
        assert!(pt_call.contains(&"--phase".to_string()));
        assert!(pt_call.contains(&"flow-complete".to_string()));
    }

    // --- error paths ---

    #[test]
    fn preflight_pr_view_failure_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK),
            fail("Could not resolve to a Pull Request"),
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_phase_transition_error_returned() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![fail("state file not found")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("phase transition"));
    }

    #[test]
    fn preflight_phase_transition_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok("not json")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_corrupt_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("test-feature.json"), "not json{{{").unwrap();

        let runner = mock_runner(vec![]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("parse"));
    }

    #[test]
    fn preflight_fetch_error_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK),
            ok("OPEN"),
            fail("Could not resolve host"),
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_push_failure_after_merge() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK),
            ok("OPEN"),
            ok_empty(),              // fetch
            fail(""),                // merge-base not ancestor
            ok("Merge made"),        // merge ok
            fail("remote rejected"), // push fails
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("push"));
    }

    #[test]
    fn preflight_merge_error_non_conflict() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![
            ok(PT_ENTER_OK),
            ok("OPEN"),
            ok_empty(),
            fail(""), // merge-base not ancestor
            fail("merge failed"),
            ok_empty(), // porcelain clean
        ]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_unexpected_pr_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("DRAFT")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("unexpected"));
    }

    #[test]
    fn preflight_no_branch_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner = mock_runner(vec![]);
        let result = preflight_inner(None, false, false, dir.path(), "/fake/bin/flow", &runner);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("branch"));
    }

    #[test]
    fn preflight_empty_branch_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner = mock_runner(vec![]);
        let result = preflight_inner(
            Some(""),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_timeout_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![err("Timed out after 30s")]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "error");
    }

    #[test]
    fn preflight_result_includes_worktree_when_state_present() {
        let dir = tempfile::tempdir().unwrap();
        setup_state(dir.path(), "test-feature", "complete", None);

        let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert!(result.get("worktree").is_some());
        assert_eq!(result["pr_number"], 42);
        assert!(result["pr_url"]
            .as_str()
            .unwrap()
            .contains("github.com/test/test/pull/42"));
    }

    #[test]
    fn preflight_inferred_result_omits_worktree() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".flow-states")).unwrap();

        let runner = mock_runner(vec![ok("OPEN"), ok_empty(), ok_empty()]);

        let result = preflight_inner(
            Some("test-feature"),
            false,
            false,
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["inferred"], true);
        assert!(result.get("worktree").is_none());
        assert!(result.get("pr_number").is_none());
    }
}
