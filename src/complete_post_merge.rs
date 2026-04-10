//! Consolidated Complete phase post-merge.
//!
//! Absorbs Steps 7 + 9 + 10: phase completion, PR body render, issues summary,
//! close issues, summary generation, label removal, auto-close parents, and
//! Slack notification. All operations are best-effort.
//!
//! Usage: bin/flow complete-post-merge --pr <N> --state-file <path> --branch <name>

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Map, Value};

use crate::git::project_root;
use crate::lock::mutate_state;
use crate::utils::bin_flow_path;

const LOCAL_TIMEOUT: u64 = 30;
const NETWORK_TIMEOUT: u64 = 60;
const POST_MERGE_STEP: i64 = 6;

type CmdResult = Result<(i32, String, String), String>;

#[derive(Parser, Debug)]
#[command(
    name = "complete-post-merge",
    about = "FLOW Complete phase post-merge operations"
)]
pub struct Args {
    /// PR number
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
    /// Branch name
    #[arg(long, required = true)]
    pub branch: String,
}

/// Run a subprocess command with a timeout. `args[0]` is the program.
///
/// Drains stdout and stderr in spawned threads to prevent pipe buffer
/// deadlock — children writing >64KB to a piped stream would otherwise
/// block forever when the kernel buffer fills and `try_wait()` would
/// never observe the child exiting. See `.claude/rules/rust-port-parity.md`
/// "Subprocess Timeout Parity".
fn run_cmd_with_timeout(args: &[&str], timeout_secs: u64) -> CmdResult {
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

/// Parse JSON from stdout. Returns (parsed_value, parse_error).
fn parse_json_or(stdout: &str) -> (Option<Value>, Option<String>) {
    match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Core post-merge logic with injectable runner. Best-effort throughout.
pub fn post_merge_inner(
    pr_number: i64,
    state_file: &str,
    branch: &str,
    root: &Path,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
    let state_path = Path::new(state_file);

    // Initialize result with default fields (preserve_order maintains this order)
    let mut result: Map<String, Value> = Map::new();
    result.insert("status".to_string(), json!("ok"));
    result.insert("formatted_time".to_string(), json!(""));
    result.insert("cumulative_seconds".to_string(), json!(0));
    result.insert("summary".to_string(), json!(""));
    result.insert("issues_links".to_string(), json!(""));
    result.insert("banner_line".to_string(), json!(""));
    result.insert("closed_issues".to_string(), json!([]));
    result.insert("parents_closed".to_string(), json!([]));
    result.insert("slack".to_string(), json!({"status": "skipped"}));
    let mut failures: Map<String, Value> = Map::new();

    // Read state for slack_thread_ts and repo (tolerate corrupt JSON)
    let state: Value = if state_path.exists() {
        match std::fs::read_to_string(state_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(json!({})),
            Err(_) => json!({}),
        }
    } else {
        json!({})
    };

    // Filter repo: null and empty are both falsy (matches Python truthy check)
    let repo: Option<String> = state
        .get("repo")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // --- Step 7: Archive artifacts to PR ---

    // Set step counter
    if state_path.exists() {
        match mutate_state(state_path, |s| {
            if !(s.is_object() || s.is_null()) {
                return;
            }
            s["complete_step"] = json!(POST_MERGE_STEP);
        }) {
            Ok(_) => {}
            Err(_) => {
                failures.insert(
                    "step_counter".to_string(),
                    json!("could not update step counter"),
                );
            }
        }
    }

    // Phase transition complete
    let pt_args = [
        bin_flow,
        "phase-transition",
        "--phase",
        "flow-complete",
        "--action",
        "complete",
        "--next-phase",
        "flow-complete",
        "--branch",
        branch,
    ];
    match runner(&pt_args, NETWORK_TIMEOUT) {
        Err(e) => {
            failures.insert("phase_transition".to_string(), json!(e));
        }
        Ok((_code, stdout, stderr)) => {
            let (parsed, parse_err) = parse_json_or(&stdout);
            match parsed.as_ref() {
                Some(pt_data) if pt_data.get("status").and_then(|v| v.as_str()) == Some("ok") => {
                    let formatted_time = pt_data
                        .get("formatted_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let cumulative_seconds = pt_data
                        .get("cumulative_seconds")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    result.insert("formatted_time".to_string(), json!(formatted_time));
                    result.insert("cumulative_seconds".to_string(), json!(cumulative_seconds));
                }
                _ => {
                    // Python: pt_err or pt_result.stderr.strip()
                    let msg = parse_err.unwrap_or_else(|| stderr.trim().to_string());
                    failures.insert("phase_transition".to_string(), json!(msg));
                }
            }
        }
    }

    // Render PR body — pass state_file explicitly because render-pr-body's
    // auto-detection uses current_branch(), which returns "main" when the
    // Complete skill runs from the project root after merge.
    let pr_str = pr_number.to_string();
    let render_args = [
        bin_flow,
        "render-pr-body",
        "--pr",
        &pr_str,
        "--state-file",
        state_file,
    ];
    match runner(&render_args, NETWORK_TIMEOUT) {
        Err(e) => {
            failures.insert("render_pr_body".to_string(), json!(e));
        }
        Ok((code, _, stderr)) => {
            if code != 0 {
                failures.insert("render_pr_body".to_string(), json!(stderr.trim()));
            }
        }
    }

    // Format issues summary
    let issues_output_path = root
        .join(".flow-states")
        .join(format!("{}-issues.md", branch));
    let issues_output = issues_output_path.to_string_lossy().to_string();
    let iss_args = [
        bin_flow,
        "format-issues-summary",
        "--state-file",
        state_file,
        "--output",
        &issues_output,
    ];
    if let Ok((_code, stdout, _stderr)) = runner(&iss_args, LOCAL_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(iss_data) = parsed {
            if iss_data.get("has_issues").and_then(|v| v.as_bool()) == Some(true) {
                let banner = iss_data
                    .get("banner_line")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                result.insert("banner_line".to_string(), json!(banner));
            }
        }
    }
    // Transport errors on format-issues-summary are silently ignored (Python matches)

    // --- Step 9: Close referenced issues ---

    let close_args = [bin_flow, "close-issues", "--state-file", state_file];
    let mut closed_issues: Vec<Value> = Vec::new();
    if let Ok((_code, stdout, _stderr)) = runner(&close_args, NETWORK_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(close_data) = parsed {
            if let Some(closed_arr) = close_data.get("closed").and_then(|v| v.as_array()) {
                closed_issues = closed_arr.clone();
            }
        }
    }
    result.insert("closed_issues".to_string(), json!(closed_issues.clone()));

    // Write closed-issues file if non-empty
    if !closed_issues.is_empty() {
        let closed_path = root
            .join(".flow-states")
            .join(format!("{}-closed-issues.json", branch));
        let closed_json =
            serde_json::to_string(&closed_issues).unwrap_or_else(|_| "[]".to_string());
        if let Err(e) = std::fs::write(&closed_path, closed_json) {
            failures.insert("closed_issues_file".to_string(), json!(e.to_string()));
        }
    }

    // --- Step 10: Parallel post-merge operations ---

    // Format complete summary
    let closed_file_path_buf = root
        .join(".flow-states")
        .join(format!("{}-closed-issues.json", branch));
    let closed_file_path = closed_file_path_buf.to_string_lossy().to_string();
    let mut sum_args: Vec<&str> = vec![
        bin_flow,
        "format-complete-summary",
        "--state-file",
        state_file,
    ];
    if !closed_issues.is_empty() {
        sum_args.push("--closed-issues-file");
        sum_args.push(&closed_file_path);
    }
    if let Ok((_code, stdout, _stderr)) = runner(&sum_args, LOCAL_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(sum_data) = parsed {
            if sum_data.get("status").and_then(|v| v.as_str()) == Some("ok") {
                let summary = sum_data
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let issues_links = sum_data
                    .get("issues_links")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                result.insert("summary".to_string(), json!(summary));
                result.insert("issues_links".to_string(), json!(issues_links));
            }
        }
    }
    // Transport errors on format-complete-summary are silently ignored

    // Remove In-Progress labels
    let label_args = [
        bin_flow,
        "label-issues",
        "--state-file",
        state_file,
        "--remove",
    ];
    match runner(&label_args, NETWORK_TIMEOUT) {
        Err(e) => {
            failures.insert("label_issues".to_string(), json!(e));
        }
        Ok((code, _, stderr)) => {
            if code != 0 {
                failures.insert("label_issues".to_string(), json!(stderr.trim()));
            }
        }
    }

    // Auto-close parent issues for each closed issue
    let mut parents_closed: Vec<i64> = Vec::new();
    if let Some(ref repo_str) = repo {
        for issue in &closed_issues {
            if let Some(issue_num) = issue.get("number").and_then(|v| v.as_i64()) {
                let issue_num_str = issue_num.to_string();
                let acp_args = [
                    bin_flow,
                    "auto-close-parent",
                    "--repo",
                    repo_str.as_str(),
                    "--issue-number",
                    &issue_num_str,
                ];
                if let Ok((_code, stdout, _stderr)) = runner(&acp_args, NETWORK_TIMEOUT) {
                    let (parsed, _) = parse_json_or(&stdout);
                    if let Some(acp_data) = parsed {
                        let parent_closed = acp_data
                            .get("parent_closed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let milestone_closed = acp_data
                            .get("milestone_closed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if parent_closed || milestone_closed {
                            parents_closed.push(issue_num);
                        }
                    }
                }
            }
        }
    }
    result.insert("parents_closed".to_string(), json!(parents_closed));

    // Slack notification — filter null and empty string (Python falsy equivalence)
    let slack_thread_ts: Option<String> = state
        .get("slack_thread_ts")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    if let Some(ref thread_ts) = slack_thread_ts {
        let msg = format!("Phase 6: Complete finished for PR #{}", pr_number);
        let slack_args = [
            bin_flow,
            "notify-slack",
            "--phase",
            "flow-complete",
            "--message",
            &msg,
            "--thread-ts",
            thread_ts.as_str(),
        ];
        match runner(&slack_args, NETWORK_TIMEOUT) {
            Err(e) => {
                result.insert(
                    "slack".to_string(),
                    json!({"status": "error", "message": e}),
                );
            }
            Ok((_code, stdout, _stderr)) => {
                let (parsed, _) = parse_json_or(&stdout);
                match parsed {
                    Some(slack_data) => {
                        // Record notification if successful
                        let status_ok =
                            slack_data.get("status").and_then(|v| v.as_str()) == Some("ok");
                        let ts_opt = slack_data
                            .get("ts")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                        result.insert("slack".to_string(), slack_data);
                        if status_ok {
                            if let Some(ts) = ts_opt {
                                let add_args = [
                                    bin_flow,
                                    "add-notification",
                                    "--phase",
                                    "flow-complete",
                                    "--ts",
                                    ts.as_str(),
                                    "--thread-ts",
                                    thread_ts.as_str(),
                                    "--message",
                                    &msg,
                                ];
                                // Fire and forget — Python ignores the result
                                let _ = runner(&add_args, LOCAL_TIMEOUT);
                            }
                        }
                    }
                    None => {
                        result.insert(
                            "slack".to_string(),
                            json!({"status": "error", "message": "invalid slack response"}),
                        );
                    }
                }
            }
        }
    }

    result.insert("failures".to_string(), Value::Object(failures));
    Value::Object(result)
}

/// Production wrapper.
pub fn post_merge(pr_number: i64, state_file: &str, branch: &str) -> Value {
    let root = project_root();
    post_merge_inner(
        pr_number,
        state_file,
        branch,
        &root,
        &bin_flow_path(),
        &run_cmd_with_timeout,
    )
}

/// CLI entry point. Always exits 0 (best-effort — matches Python main()).
pub fn run(args: Args) {
    let result = post_merge(args.pr, &args.state_file, &args.branch);
    println!("{}", result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    const PT_COMPLETE_OK: &str = r#"{"status": "ok", "phase": "flow-complete", "action": "complete", "cumulative_seconds": 45, "formatted_time": "<1m", "next_phase": "flow-complete", "continue_action": "invoke"}"#;
    const RENDER_PR_OK: &str = r#"{"status": "ok", "sections": ["What"]}"#;
    const ISSUES_SUMMARY_NO_ISSUES: &str =
        r#"{"status": "ok", "has_issues": false, "banner_line": "", "table": ""}"#;
    const ISSUES_SUMMARY_WITH_ISSUES: &str = r#"{"status": "ok", "has_issues": true, "banner_line": "Issues filed: 1 (Flaky Test: 1)", "table": "| Label | Title |"}"#;
    const CLOSE_ISSUES_EMPTY: &str = r#"{"status": "ok", "closed": [], "failed": []}"#;
    const CLOSE_ISSUES_WITH_CLOSED: &str = r#"{"status": "ok", "closed": [{"number": 100, "url": "https://github.com/test/test/issues/100"}], "failed": []}"#;
    const SUMMARY_OK: &str =
        r#"{"status": "ok", "summary": "test summary", "total_seconds": 300, "issues_links": ""}"#;
    const LABEL_OK: &str = r#"{"status": "ok", "labeled": [100], "failed": []}"#;
    const AUTO_CLOSE_OK: &str =
        r#"{"status": "ok", "parent_closed": false, "milestone_closed": false}"#;
    const SLACK_OK: &str = r#"{"status": "ok", "ts": "1234567890.123456"}"#;
    const ADD_NOTIFICATION_OK: &str = r#"{"status": "ok", "notification_count": 1}"#;

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

    fn fail(stderr: &str) -> CmdResult {
        Ok((1, String::new(), stderr.to_string()))
    }

    fn err(msg: &str) -> CmdResult {
        Err(msg.to_string())
    }

    /// Setup fixture: create root/.flow-states/ and write state file there.
    fn setup(
        dir: &Path,
        branch: &str,
        slack_thread_ts: Option<&str>,
        repo: Option<&str>,
    ) -> PathBuf {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let mut state = json!({
            "schema_version": 1,
            "branch": branch,
            "pr_number": 42,
            "pr_url": "https://github.com/test/test/pull/42",
            "prompt": "work on issue #100",
            "complete_step": 5,
            "phases": {
                "flow-start": {"status": "complete"},
                "flow-plan": {"status": "complete"},
                "flow-code": {"status": "complete"},
                "flow-code-review": {"status": "complete"},
                "flow-learn": {"status": "complete"},
                "flow-complete": {"status": "in_progress"}
            }
        });
        if let Some(ts) = slack_thread_ts {
            state["slack_thread_ts"] = json!(ts);
        }
        if let Some(r) = repo {
            state["repo"] = json!(r);
        } else {
            state["repo"] = json!("test/test");
        }
        let state_path = state_dir.join(format!("{}.json", branch));
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
        state_path
    }

    // --- happy paths ---

    #[test]
    fn happy_path_no_issues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["formatted_time"], "<1m");
        assert_eq!(result["summary"], "test summary");
        assert_eq!(result["cumulative_seconds"], 45);
    }

    #[test]
    fn happy_path_with_closed_issues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(AUTO_CLOSE_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["closed_issues"].as_array().unwrap().len(), 1);

        // Closed issues file written to disk
        let closed_path = dir
            .path()
            .join(".flow-states")
            .join("test-feature-closed-issues.json");
        assert!(closed_path.exists());
        let content: Value =
            serde_json::from_str(&fs::read_to_string(&closed_path).unwrap()).unwrap();
        assert_eq!(content[0]["number"], 100);
    }

    #[test]
    fn individual_failure_continues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            fail("gh error"), // label-issues fails
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert!(result["failures"]
            .as_object()
            .unwrap()
            .contains_key("label_issues"));
    }

    // --- slack ---

    #[test]
    fn slack_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            // no slack calls expected
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "skipped");
    }

    #[test]
    fn slack_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", Some("1234.5678"), None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(SLACK_OK),
            ok(ADD_NOTIFICATION_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "ok");
        assert_eq!(result["slack"]["ts"], "1234567890.123456");
    }

    #[test]
    fn slack_failure_continues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", Some("1234.5678"), None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(r#"{"status": "error", "message": "token expired"}"#),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "error");
        // Overall status still ok — slack is best-effort
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn slack_invalid_response() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", Some("1234.5678"), None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok("not json"),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "error");
        assert!(result["slack"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("invalid"));
    }

    #[test]
    fn slack_thread_ts_empty_string_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", Some(""), None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            // no slack calls — empty thread_ts is falsy
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "skipped");
    }

    #[test]
    fn slack_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", Some("1234.5678"), None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            err("Timed out after 60s"),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["slack"]["status"], "error");
        assert_eq!(result["status"], "ok");
    }

    // --- phase-transition invocation ---

    #[test]
    fn phase_transition_called_with_next_phase() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
        let runner = tracking_runner(
            vec![
                ok(PT_COMPLETE_OK),
                ok(RENDER_PR_OK),
                ok(ISSUES_SUMMARY_NO_ISSUES),
                ok(CLOSE_ISSUES_EMPTY),
                ok(SUMMARY_OK),
                ok(LABEL_OK),
            ],
            calls.clone(),
        );

        post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
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
        assert!(pt_call.contains(&"--next-phase".to_string()));
        assert!(pt_call.contains(&"flow-complete".to_string()));
        assert!(pt_call.contains(&"--branch".to_string()));
        assert!(pt_call.contains(&"test-feature".to_string()));
    }

    #[test]
    fn render_pr_body_called_with_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
        let runner = tracking_runner(
            vec![
                ok(PT_COMPLETE_OK),
                ok(RENDER_PR_OK),
                ok(ISSUES_SUMMARY_NO_ISSUES),
                ok(CLOSE_ISSUES_EMPTY),
                ok(SUMMARY_OK),
                ok(LABEL_OK),
            ],
            calls.clone(),
        );

        post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let render_call = calls
            .borrow()
            .iter()
            .find(|c| c.iter().any(|a| a == "render-pr-body"))
            .cloned()
            .expect("render-pr-body call not found");
        assert!(
            render_call.contains(&"--state-file".to_string()),
            "render-pr-body must receive --state-file arg"
        );
        assert!(
            render_call.contains(&state_path.to_str().unwrap().to_string()),
            "render-pr-body must receive the state file path"
        );
    }

    // --- step counter persistence ---

    #[test]
    fn step_counters_updated() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["complete_step"], json!(6));
    }

    // --- error paths ---

    #[test]
    fn phase_transition_failure_captured() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            fail(r#"{"status": "error", "message": "bad state"}"#),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        // Best-effort — still ok overall
        assert_eq!(result["status"], "ok");
        assert!(result["failures"]
            .as_object()
            .unwrap()
            .contains_key("phase_transition"));
    }

    #[test]
    fn corrupt_state_file_handled() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test-feature.json");
        fs::write(&state_path, "not valid json{{{").unwrap();

        let runner = mock_runner(vec![
            fail(r#"{"status": "error"}"#),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        // Does not crash
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn issues_summary_with_issues_populates_banner() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_WITH_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["banner_line"], "Issues filed: 1 (Flaky Test: 1)");
    }

    #[test]
    fn closed_issues_file_write_error() {
        let dir = tempfile::tempdir().unwrap();
        // Create state file at a path OUTSIDE .flow-states so state ops work,
        // but do NOT create .flow-states/ under root — the closed-issues file
        // write will fail with ENOENT.
        let state_path = dir.path().join("state.json");
        let state = json!({
            "schema_version": 1,
            "branch": "test-feature",
            "pr_number": 42,
            "repo": "test/test",
            "phases": {}
        });
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(AUTO_CLOSE_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert!(result["failures"]
            .as_object()
            .unwrap()
            .contains_key("closed_issues_file"));
    }

    #[test]
    fn parent_closed_populates_parents_closed() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(r#"{"status": "ok", "parent_closed": true, "milestone_closed": false}"#),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let parents: Vec<i64> = result["parents_closed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(parents, vec![100]);
    }

    #[test]
    fn milestone_closed_also_populates_parents_closed() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            ok(r#"{"status": "ok", "parent_closed": false, "milestone_closed": true}"#),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        let parents: Vec<i64> = result["parents_closed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(parents, vec![100]);
    }

    #[test]
    fn repo_null_skips_auto_close_parent() {
        let dir = tempfile::tempdir().unwrap();
        // Write state with repo:null
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state = json!({
            "schema_version": 1,
            "branch": "test-feature",
            "pr_number": 42,
            "repo": null,
            "phases": {}
        });
        let state_path = state_dir.join("test-feature.json");
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            // NO auto-close-parent call because repo is null
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["parents_closed"], json!([]));
    }

    #[test]
    fn repo_empty_string_skips_auto_close_parent() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, Some(""));

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_WITH_CLOSED),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
            // NO auto-close-parent call
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["parents_closed"], json!([]));
    }

    #[test]
    fn timeout_handling_all_calls_fail() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            err("Timed out after 60s"), // pt
            err("Timed out after 60s"), // render
            err("Timed out after 30s"), // issues
            err("Timed out after 60s"), // close
            err("Timed out after 30s"), // summary
            err("Timed out after 60s"), // label
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        // Best-effort — overall ok, failures populated
        assert_eq!(result["status"], "ok");
        let failures = result["failures"].as_object().unwrap();
        assert!(failures.contains_key("phase_transition"));
        assert!(failures.contains_key("render_pr_body"));
        assert!(failures.contains_key("label_issues"));
    }

    #[test]
    fn render_pr_body_failure_captured() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            fail("gh API error"),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert!(result["failures"]
            .as_object()
            .unwrap()
            .contains_key("render_pr_body"));
    }

    #[test]
    fn missing_state_file_still_produces_result() {
        let dir = tempfile::tempdir().unwrap();
        // Do not create any state file
        let state_path = dir.path().join(".flow-states").join("test-feature.json");
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        // Note: state_path does not exist as a file

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        // slack defaults to skipped because state is empty dict (no thread_ts)
        assert_eq!(result["slack"]["status"], "skipped");
    }

    #[test]
    fn non_object_state_file_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test-feature.json");
        // Array instead of object — mutate_state closure must guard
        fs::write(&state_path, "[1, 2, 3]").unwrap();

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        // Should not panic
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn close_issues_parse_failure_continues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = setup(dir.path(), "test-feature", None, None);

        let runner = mock_runner(vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok("not json"), // close-issues parse fails
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ]);

        let result = post_merge_inner(
            42,
            state_path.to_str().unwrap(),
            "test-feature",
            dir.path(),
            "/fake/bin/flow",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["closed_issues"], json!([]));
    }
}
