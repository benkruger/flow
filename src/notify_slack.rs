//! Port of lib/notify-slack.py — post messages to Slack via curl.
//!
//! Usage:
//!   bin/flow notify-slack --phase <phase> --message <text> [--thread-ts <ts>]
//!                         [--feature <name>] [--pr-url <url>]
//!
//! Output (JSON to stdout):
//!   Success:  {"status": "ok", "ts": "1234567890.123456"}
//!   Skipped:  {"status": "skipped", "reason": "no slack config"}
//!   Error:    {"status": "error", "message": "..."}

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::phase_config::phase_names;

const SLACK_API_URL: &str = "https://slack.com/api/chat.postMessage";
const TOKEN_ENV: &str = "CLAUDE_PLUGIN_CONFIG_slack_bot_token";
const CHANNEL_ENV: &str = "CLAUDE_PLUGIN_CONFIG_slack_channel";
const CURL_TIMEOUT: u64 = 15;

#[derive(Parser, Debug)]
#[command(name = "notify-slack", about = "Post to Slack")]
pub struct Args {
    /// Phase name
    #[arg(long)]
    pub phase: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Thread timestamp for replies
    #[arg(long = "thread-ts")]
    pub thread_ts: Option<String>,
    /// Feature name
    #[arg(long)]
    pub feature: Option<String>,
    /// PR URL
    #[arg(long = "pr-url")]
    pub pr_url: Option<String>,
}

/// Slack configuration from env vars.
pub struct SlackConfig {
    pub bot_token: String,
    pub channel: String,
}

/// Read slack config from env vars. Returns None if not configured.
pub fn read_slack_config() -> Option<SlackConfig> {
    let bot_token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let channel = std::env::var(CHANNEL_ENV).unwrap_or_default();
    if bot_token.is_empty() || channel.is_empty() {
        return None;
    }
    Some(SlackConfig { bot_token, channel })
}

/// Format a Slack notification message.
pub fn format_message(
    phase: &str,
    message: &str,
    feature: Option<&str>,
    pr_url: Option<&str>,
) -> String {
    let names = phase_names();
    let phase_name = names
        .get(phase)
        .map(|s| s.as_str())
        .unwrap_or(phase);
    let mut parts = vec![format!("*{}*: {}", phase_name, message)];
    if let Some(f) = feature {
        parts.push(format!("Feature: {}", f));
    }
    if let Some(url) = pr_url {
        parts.push(format!("PR: {}", url));
    }
    parts.join("\n")
}

/// Post a message to Slack via curl with injectable runner for testing.
pub fn post_message_inner(
    bot_token: &str,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
    curl: &dyn Fn(&[&str], u64) -> Result<(i32, String, String), String>,
) -> Value {
    let mut payload = json!({"channel": channel, "text": text});
    if let Some(ts) = thread_ts {
        payload["thread_ts"] = json!(ts);
    }
    let payload_str = payload.to_string();
    let auth_header = format!("Authorization: Bearer {}", bot_token);

    match curl(
        &[
            "-s",
            "-X", "POST",
            SLACK_API_URL,
            "-H", &auth_header,
            "-H", "Content-Type: application/json; charset=utf-8",
            "-d", &payload_str,
        ],
        CURL_TIMEOUT,
    ) {
        Err(e) => json!({"status": "error", "message": e}),
        Ok((code, stdout, stderr)) => {
            if code != 0 {
                let error = if stderr.trim().is_empty() {
                    "curl failed".to_string()
                } else {
                    stderr.trim().to_string()
                };
                return json!({"status": "error", "message": error});
            }

            let response: Value = match serde_json::from_str(&stdout) {
                Ok(v) => v,
                Err(_) => {
                    return json!({"status": "error", "message": "Invalid JSON response from Slack"});
                }
            };

            if response.get("ok") != Some(&json!(true)) {
                let error = response
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return json!({"status": "error", "message": format!("Slack API error: {}", error)});
            }

            let ts = response
                .get("ts")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            json!({"status": "ok", "ts": ts})
        }
    }
}

/// Run curl as a subprocess with timeout.
fn run_curl_with_timeout(
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    let mut child = Command::new("curl")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn curl: {}", e))?;

    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let code = output.status.code().unwrap_or(1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok((code, stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Timeout posting to Slack".to_string());
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                std::thread::sleep(poll_interval.min(remaining));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Post a message to Slack via real curl subprocess.
pub fn post_message(
    bot_token: &str,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
) -> Value {
    post_message_inner(bot_token, channel, text, thread_ts, &run_curl_with_timeout)
}

/// Core notification logic. Returns result Value.
pub fn notify(args: &Args) -> Value {
    let config = match read_slack_config() {
        None => return json!({"status": "skipped", "reason": "no slack config"}),
        Some(c) => c,
    };

    let text = format_message(
        &args.phase,
        &args.message,
        args.feature.as_deref(),
        args.pr_url.as_deref(),
    );
    post_message(
        &config.bot_token,
        &config.channel,
        &text,
        args.thread_ts.as_deref(),
    )
}

pub fn run(args: Args) {
    let result = notify(&args);
    println!("{}", result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

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

    // --- read_slack_config ---

    #[test]
    fn read_config_from_env() {
        unsafe {
            std::env::set_var(TOKEN_ENV, "xoxb-test-token");
            std::env::set_var(CHANNEL_ENV, "C12345");
        }
        let config = read_slack_config().unwrap();
        assert_eq!(config.bot_token, "xoxb-test-token");
        assert_eq!(config.channel, "C12345");
        unsafe {
            std::env::remove_var(TOKEN_ENV);
            std::env::remove_var(CHANNEL_ENV);
        }
    }

    #[test]
    fn read_config_missing_token() {
        unsafe {
            std::env::remove_var(TOKEN_ENV);
            std::env::set_var(CHANNEL_ENV, "C12345");
        }
        assert!(read_slack_config().is_none());
        unsafe {
            std::env::remove_var(CHANNEL_ENV);
        }
    }

    #[test]
    fn read_config_missing_channel() {
        unsafe {
            std::env::set_var(TOKEN_ENV, "xoxb-test");
            std::env::remove_var(CHANNEL_ENV);
        }
        assert!(read_slack_config().is_none());
        unsafe {
            std::env::remove_var(TOKEN_ENV);
        }
    }

    #[test]
    fn read_config_both_missing() {
        unsafe {
            std::env::remove_var(TOKEN_ENV);
            std::env::remove_var(CHANNEL_ENV);
        }
        assert!(read_slack_config().is_none());
    }

    #[test]
    fn read_config_empty_values() {
        unsafe {
            std::env::set_var(TOKEN_ENV, "");
            std::env::set_var(CHANNEL_ENV, "");
        }
        assert!(read_slack_config().is_none());
        unsafe {
            std::env::remove_var(TOKEN_ENV);
            std::env::remove_var(CHANNEL_ENV);
        }
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

    // --- post_message_inner ---

    #[test]
    fn post_message_success() {
        let slack_response = json!({"ok": true, "ts": "1234567890.123456"});
        let curl = mock_curl(vec![Ok((
            0,
            slack_response.to_string(),
            String::new(),
        ))]);

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
        // Verify thread_ts was in the payload
        let args = call_args.borrow();
        let payload_arg = args[0].iter().find(|a| a.contains("thread_ts"));
        assert!(payload_arg.is_some());
    }

    #[test]
    fn post_message_slack_error() {
        let slack_response = json!({"ok": false, "error": "channel_not_found"});
        let curl = mock_curl(vec![Ok((
            0,
            slack_response.to_string(),
            String::new(),
        ))]);

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
}
