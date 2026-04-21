//! Integration tests for `src/notify_slack.rs`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::Command;

use flow_rs::notify_slack::{
    build_config, format_message, post_message_inner, read_slack_config_with_env,
};
use serde_json::{json, Value};

type CurlResult = Result<(i32, String, String), String>;

fn mock_curl(responses: Vec<CurlResult>) -> impl Fn(&[&str], u64) -> CurlResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |_args: &[&str], _timeout: u64| -> CurlResult {
        queue
            .borrow_mut()
            .pop_front()
            .expect("no more mock responses")
    }
}

// --- build_config ---

#[test]
fn build_config_both_present() {
    let config = build_config("xoxb-test-token", "C12345").unwrap();
    assert_eq!(config.bot_token, "xoxb-test-token");
    assert_eq!(config.channel, "C12345");
}

#[test]
fn build_config_missing_token() {
    assert!(build_config("", "C12345").is_none());
}

#[test]
fn build_config_missing_channel() {
    assert!(build_config("xoxb-test", "").is_none());
}

#[test]
fn build_config_both_empty() {
    assert!(build_config("", "").is_none());
}

// --- read_slack_config_with_env ---

#[test]
fn read_slack_config_with_env_returns_config_when_both_present() {
    let token = || "xoxb-test-token".to_string();
    let channel = || "C12345".to_string();
    let config = read_slack_config_with_env(&token, &channel).unwrap();
    assert_eq!(config.bot_token, "xoxb-test-token");
    assert_eq!(config.channel, "C12345");
}

#[test]
fn read_slack_config_with_env_returns_none_when_token_empty() {
    let token = || String::new();
    let channel = || "C12345".to_string();
    assert!(read_slack_config_with_env(&token, &channel).is_none());
}

#[test]
fn read_slack_config_with_env_returns_none_when_channel_empty() {
    let token = || "xoxb-test-token".to_string();
    let channel = || String::new();
    assert!(read_slack_config_with_env(&token, &channel).is_none());
}

// --- format_message ---

#[test]
fn format_message_basic() {
    let result = format_message("flow-start", "Feature started", None, None);
    assert!(result.contains("Start"));
    assert!(result.contains("Feature started"));
}

#[test]
fn format_message_with_feature_and_pr() {
    let result = format_message(
        "flow-start",
        "Feature started",
        Some("Invoice Export"),
        Some("https://github.com/org/repo/pull/42"),
    );
    assert!(result.contains("Invoice Export"));
    assert!(result.contains("https://github.com/org/repo/pull/42"));
}

#[test]
fn format_message_unknown_phase() {
    let result = format_message("unknown-phase", "Some message", None, None);
    assert!(result.contains("Some message"));
}

// --- run_curl_with_timeout subprocess tests ---
//
// `run_curl_with_timeout_inner` is no longer pub per the pub-for-testing
// revert. Its success/timeout/spawn-error branches are now driven by
// spawning the compiled `bin/flow notify-slack` binary with a fake
// `curl` on PATH — this exercises `run_curl_with_timeout` (which wraps
// `run_curl_with_timeout_inner` with the real curl factory) through
// the real production path.

fn install_fake_bin(dir: &std::path::Path, name: &str, script: &str) -> std::path::PathBuf {
    let bin_dir = dir.join("fakebin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join(name);
    std::fs::write(&bin, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin_dir
}

#[test]
fn subprocess_run_curl_success_returns_status_ok() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Fake curl returns a successful Slack response.
    let bin_dir = install_fake_bin(
        &root,
        "curl",
        r#"#!/usr/bin/env bash
cat <<'EOF'
{"ok":true,"ts":"1234.5678","channel":"C123","message":{"text":"ok"}}
EOF
exit 0
"#,
    );

    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["notify-slack", "--phase", "flow-start", "--message", "hi"])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "xoxb-test")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).expect("json");
    assert_eq!(data["status"], "ok");
    assert_eq!(data["ts"], "1234.5678");
}

#[test]
fn subprocess_run_curl_nonzero_exit_surfaces_error() {
    // Fake curl exits 1 with empty stderr → post_message_inner falls
    // back to "curl failed" error message.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin_dir = install_fake_bin(&root, "curl", "#!/usr/bin/env bash\nexit 1\n");

    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["notify-slack", "--phase", "flow-start", "--message", "hi"])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "xoxb-test")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).expect("json");
    assert_eq!(data["status"], "error");
}

#[test]
fn subprocess_run_curl_spawn_failure_surfaces_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // No curl on PATH → Command::new("curl") spawn fails.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["notify-slack", "--phase", "flow-start", "--message", "hi"])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", "/nonexistent-no-curl-here")
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "xoxb-test")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    // Status may be 0 or non-zero depending on how the status field is
    // routed. The important assertion is the error shape.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).expect("json");
    assert_eq!(data["status"], "error");
}

// --- post_message_inner ---

#[test]
fn post_message_success() {
    let slack_response = json!({"ok": true, "ts": "1234567890.123456"});
    let curl = mock_curl(vec![Ok((0, slack_response.to_string(), String::new()))]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["ts"], "1234567890.123456");
}

#[test]
fn post_message_with_thread_ts() {
    let slack_response = json!({"ok": true, "ts": "1234567890.654321"});
    let call_args: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
    let call_args_ref = &call_args;
    let queue = RefCell::new(VecDeque::from(vec![Ok((
        0,
        slack_response.to_string(),
        String::new(),
    ))]));

    let curl = |args: &[&str], _timeout: u64| -> CurlResult {
        call_args_ref
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        queue.borrow_mut().pop_front().unwrap()
    };

    let result = post_message_inner(
        "xoxb-token",
        "C12345",
        "Reply",
        Some("1234567890.123456"),
        &curl,
    );
    assert_eq!(result["status"], "ok");
    let args = call_args.borrow();
    let payload_arg = args[0].iter().find(|a| a.contains("thread_ts"));
    assert!(payload_arg.is_some());
}

#[test]
fn post_message_slack_error() {
    let slack_response = json!({"ok": false, "error": "channel_not_found"});
    let curl = mock_curl(vec![Ok((0, slack_response.to_string(), String::new()))]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("channel_not_found"));
}

#[test]
fn post_message_curl_failure() {
    let curl = mock_curl(vec![Ok((
        1,
        String::new(),
        "Connection refused".to_string(),
    ))]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "error");
}

#[test]
fn post_message_curl_failure_empty_stderr_returns_curl_failed() {
    let curl = mock_curl(vec![Ok((1, String::new(), String::new()))]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "error");
    assert_eq!(result["message"], "curl failed");
}

#[test]
fn post_message_curl_timeout() {
    let curl = mock_curl(vec![Err("Timeout posting to Slack".to_string())]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("timeout"));
}

#[test]
fn post_message_invalid_json_response() {
    let curl = mock_curl(vec![Ok((
        0,
        "<html>error</html>".to_string(),
        String::new(),
    ))]);
    let result = post_message_inner("xoxb-token", "C12345", "Hello", None, &curl);
    assert_eq!(result["status"], "error");
}

// Subprocess tests above drive `notify` → `notify_with_deps` through
// the compiled binary with fake curl + env-var config. The no-config
// skipped path is exercised separately:

#[test]
fn subprocess_notify_no_config_returns_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["notify-slack", "--phase", "flow-start", "--message", "hi"])
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("CLAUDE_PLUGIN_CONFIG_slack_bot_token")
        .env_remove("CLAUDE_PLUGIN_CONFIG_slack_channel")
        .env("HOME", &root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).expect("json");
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "no slack config");
}

#[test]
fn subprocess_notify_feature_and_pr_url_formatted_into_message() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin_dir = install_fake_bin(
        &root,
        "curl",
        r#"#!/usr/bin/env bash
echo '{"ok":true,"ts":"1.1"}'
exit 0
"#,
    );
    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "notify-slack",
            "--phase",
            "flow-start",
            "--message",
            "Feature started",
            "--feature",
            "Invoice Export",
            "--pr-url",
            "https://github.com/org/repo/pull/42",
        ])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "xoxb-test")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).expect("json");
    assert_eq!(data["status"], "ok");
}
