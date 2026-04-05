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

/// Build config from explicit token and channel values.
/// Returns None if either is empty.
pub fn build_config(bot_token: &str, channel: &str) -> Option<SlackConfig> {
    if bot_token.is_empty() || channel.is_empty() {
        return None;
    }
    Some(SlackConfig {
        bot_token: bot_token.to_string(),
        channel: channel.to_string(),
    })
}

/// Read slack config from env vars. Returns None if not configured.
pub fn read_slack_config() -> Option<SlackConfig> {
    let bot_token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let channel = std::env::var(CHANNEL_ENV).unwrap_or_default();
    build_config(&bot_token, &channel)
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

/// Run curl as a subprocess with timeout. Thin wrapper over `run_with_timeout_inner`
/// that hardcodes the "curl" program name.
fn run_curl_with_timeout(
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    run_with_timeout_inner("curl", args, Duration::from_secs(timeout_secs))
        .map_err(|e| {
            // Preserve the existing Slack-specific timeout error message.
            if e == "timeout" {
                "Timeout posting to Slack".to_string()
            } else if e.starts_with("Failed to spawn: ") {
                format!("Failed to spawn curl: {}", &e[17..])
            } else {
                e
            }
        })
}

/// Run a subprocess with a timeout, returning (exit_code, stdout, stderr).
///
/// Drains stdout and stderr in spawned reader threads before the poll loop
/// to prevent pipe buffer deadlock on outputs larger than ~64KB. Joins reader
/// threads on every exit path (success, timeout, try_wait error).
///
/// The `program` parameter is test-injectable — production passes "curl".
fn run_with_timeout_inner(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(i32, String, String), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stdout_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stderr_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

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
                    return Err("timeout".to_string());
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

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let code = status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
    Ok((code, stdout, stderr))
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

    // --- build_config (parallel-safe, no env var mutation) ---

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

    // --- run_with_timeout_inner large-output and timeout tests (issue #875) ---
    //
    // These verify the thread-drain pattern captures output exceeding the
    // kernel pipe buffer (~64KB). The prior try_wait() + wait_with_output()
    // pattern either deadlocked on pipe-buffer fill or silently truncated
    // via ECHILD on already-reaped children.

    #[test]
    fn run_with_timeout_inner_captures_large_stdout() {
        let result = run_with_timeout_inner(
            "sh",
            &["-c", "for i in $(seq 1 20000); do echo \"line $i\"; done"],
            Duration::from_secs(10),
        );
        let (code, stdout, _) = result.expect("subprocess failed");
        assert_eq!(code, 0);
        assert!(
            stdout.contains("line 20000"),
            "last line missing — output was truncated"
        );
        assert!(
            stdout.len() > 128_000,
            "stdout truncated: {} bytes (expected > 128KB)",
            stdout.len()
        );
    }

    #[test]
    fn run_with_timeout_inner_captures_large_stderr_on_failure() {
        let result = run_with_timeout_inner(
            "sh",
            &[
                "-c",
                "for i in $(seq 1 20000); do echo \"err $i\" 1>&2; done; exit 4",
            ],
            Duration::from_secs(10),
        );
        let (code, _, stderr) = result.expect("subprocess failed");
        assert_eq!(code, 4);
        assert!(
            stderr.contains("err 20000"),
            "last stderr line missing — output was truncated"
        );
        assert!(
            stderr.len() > 128_000,
            "stderr truncated: {} bytes (expected > 128KB)",
            stderr.len()
        );
    }

    #[test]
    fn run_with_timeout_inner_enforces_timeout() {
        let start = Instant::now();
        let result = run_with_timeout_inner(
            "sh",
            &["-c", "sleep 10"],
            Duration::from_secs(2),
        );
        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "timeout");
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout not enforced: elapsed {:?}",
            elapsed
        );
    }

}
