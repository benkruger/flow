//! Integration tests for `src/notify_slack.rs`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::{Command, Stdio};

use flow_rs::notify_slack::{
    build_config, format_message, notify_with_deps, post_message_inner, read_slack_config_with_env,
    run_curl_with_timeout_inner, run_impl_main, Args, SlackConfig,
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

// --- run_curl_with_timeout_inner ---

#[test]
fn run_curl_with_timeout_inner_success_returns_output() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo ok"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let (code, stdout, stderr) = run_curl_with_timeout_inner(&["irrelevant"], 5, &factory).unwrap();
    assert_eq!(code, 0);
    assert!(stdout.contains("ok"));
    assert!(stderr.is_empty());
}

#[test]
fn run_curl_with_timeout_inner_timeout_kills_child_returns_err() {
    let factory = |_args: &[&str]| {
        Command::new("sleep")
            .arg("5")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = run_curl_with_timeout_inner(&["irrelevant"], 1, &factory).unwrap_err();
    assert!(err.to_lowercase().contains("timeout"));
}

#[test]
fn run_curl_with_timeout_inner_spawn_error_returns_err() {
    let factory = |_args: &[&str]| -> std::io::Result<std::process::Child> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such binary",
        ))
    };
    let err = run_curl_with_timeout_inner(&["irrelevant"], 5, &factory).unwrap_err();
    assert!(err.contains("no such binary") || err.contains("Failed to spawn"));
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

// --- notify_with_deps ---

fn test_args(
    phase: &str,
    message: &str,
    thread_ts: Option<&str>,
    feature: Option<&str>,
    pr_url: Option<&str>,
) -> Args {
    Args {
        phase: phase.to_string(),
        message: message.to_string(),
        thread_ts: thread_ts.map(|s| s.to_string()),
        feature: feature.map(|s| s.to_string()),
        pr_url: pr_url.map(|s| s.to_string()),
    }
}

#[test]
fn notify_with_deps_no_config_returns_skipped() {
    let args = test_args("flow-start", "Feature started", None, None, None);
    let config_reader = || None;
    let poster_called = RefCell::new(false);
    let poster = |_: &str, _: &str, _: &str, _: Option<&str>| -> Value {
        *poster_called.borrow_mut() = true;
        json!({"status": "ok"})
    };

    let result = notify_with_deps(&args, &config_reader, &poster);
    assert_eq!(result["status"], "skipped");
    assert_eq!(result["reason"], "no slack config");
    assert!(!*poster_called.borrow());
}

#[test]
fn notify_with_deps_success_formats_and_posts() {
    type PosterCall = (String, String, String, Option<String>);
    let args = test_args(
        "flow-start",
        "Feature started",
        Some("1234567890.123456"),
        None,
        None,
    );
    let config_reader = || {
        Some(SlackConfig {
            bot_token: "xoxb-test-token".to_string(),
            channel: "C12345".to_string(),
        })
    };
    let poster_calls: RefCell<Vec<PosterCall>> = RefCell::new(Vec::new());
    let poster = |bot: &str, channel: &str, text: &str, tts: Option<&str>| -> Value {
        poster_calls.borrow_mut().push((
            bot.to_string(),
            channel.to_string(),
            text.to_string(),
            tts.map(|s| s.to_string()),
        ));
        json!({"status": "ok", "ts": "5555.6666"})
    };

    let result = notify_with_deps(&args, &config_reader, &poster);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["ts"], "5555.6666");

    let calls = poster_calls.borrow();
    assert_eq!(calls.len(), 1);
    let (bot, channel, text, tts) = &calls[0];
    assert_eq!(bot, "xoxb-test-token");
    assert_eq!(channel, "C12345");
    assert!(text.contains("Feature started"));
    assert!(text.contains("Start"));
    assert_eq!(tts.as_deref(), Some("1234567890.123456"));
}

// --- run_impl_main ---

#[test]
fn notify_slack_run_impl_main_writes_skipped_json_when_unconfigured() {
    let args = test_args("flow-start", "hi", None, None, None);
    let config_reader = || None;
    let poster = |_: &str, _: &str, _: &str, _: Option<&str>| -> Value {
        json!({"status": "ok", "ts": "9.9"})
    };
    let (value, code) = run_impl_main(args, &config_reader, &poster);
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["reason"], "no slack config");
    assert_eq!(code, 0);
}

#[test]
fn notify_slack_run_impl_main_writes_ok_json_on_success() {
    let args = test_args("flow-start", "hi", None, None, None);
    let config_reader = || {
        Some(SlackConfig {
            bot_token: "xoxb".to_string(),
            channel: "C".to_string(),
        })
    };
    let poster = |_: &str, _: &str, _: &str, _: Option<&str>| -> Value {
        json!({"status": "ok", "ts": "9.9"})
    };
    let (value, code) = run_impl_main(args, &config_reader, &poster);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["ts"], "9.9");
    assert_eq!(code, 0);
}

#[test]
fn notify_with_deps_with_feature_and_pr_url() {
    let args = test_args(
        "flow-start",
        "Feature started",
        None,
        Some("Invoice Export"),
        Some("https://github.com/org/repo/pull/42"),
    );
    let config_reader = || {
        Some(SlackConfig {
            bot_token: "xoxb-test-token".to_string(),
            channel: "C12345".to_string(),
        })
    };
    let posted_text: RefCell<String> = RefCell::new(String::new());
    let poster = |_bot: &str, _channel: &str, text: &str, _tts: Option<&str>| -> Value {
        *posted_text.borrow_mut() = text.to_string();
        json!({"status": "ok", "ts": "5555.6666"})
    };

    let _ = notify_with_deps(&args, &config_reader, &poster);
    let text = posted_text.borrow();
    assert!(text.contains("Invoice Export"));
    assert!(text.contains("https://github.com/org/repo/pull/42"));
}
