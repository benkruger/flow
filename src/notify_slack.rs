//! Post messages to Slack via curl.
//!
//! Usage:
//!   bin/flow notify-slack --phase <phase> --message <text> [--thread-ts <ts>]
//!                         [--feature <name>] [--pr-url <url>]
//!
//! Output (JSON to stdout):
//!   Success:  {"status": "ok", "ts": "1234567890.123456"}
//!   Skipped:  {"status": "skipped", "reason": "no slack config"}
//!   Error:    {"status": "error", "message": "..."}
//!
//! # Public entry points
//!
//! The module exposes a two-tier layering so inline tests can drive every
//! branch without env-var mutation or real `curl` subprocesses:
//!
//! - [`notify_with_deps`] — dependency-injected core. Accepts a
//!   `config_reader` closure returning `Option<SlackConfig>` and a
//!   `poster` closure returning the Slack JSON response. Fully testable.
//! - [`notify`] — production binder that wires `notify_with_deps` to
//!   [`read_slack_config`] (env-var reader) and [`post_message_inner`]
//!   bound to [`run_curl_with_timeout`] (real curl subprocess).
//! - [`run_with_deps`] — CLI layer with an injected
//!   `writer: &mut dyn Write`. Computes the notify result and writes one
//!   JSON line. Testable via in-memory `Vec<u8>` buffers.
//! - [`run`] — production CLI entry. Wires `run_with_deps` to
//!   `std::io::stdout()` and the production closures above.
//!
//! The inner [`post_message_inner`] closure seam (injected `curl` runner)
//! predates this split and remains the existing test entry for the
//! `curl` response-parsing branches (success, 4xx/5xx, invalid JSON,
//! timeout) via the inline `mock_curl` helper.

use std::process::{Child, Command, Stdio};
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

/// Read slack config via injected env-readers. Production wraps this with
/// closures that call `std::env::var(TOKEN_ENV)` and
/// `std::env::var(CHANNEL_ENV)`. The seam exists so unit tests cover the
/// configuration build paths without `std::env::set_var` (forbidden in
/// parallel tests per `.claude/rules/testing-gotchas.md`).
pub fn read_slack_config_with_env(
    token_reader: &dyn Fn() -> String,
    channel_reader: &dyn Fn() -> String,
) -> Option<SlackConfig> {
    build_config(&token_reader(), &channel_reader())
}

/// Read slack config from env vars. Returns None if not configured.
pub fn read_slack_config() -> Option<SlackConfig> {
    read_slack_config_with_env(&|| std::env::var(TOKEN_ENV).unwrap_or_default(), &|| {
        std::env::var(CHANNEL_ENV).unwrap_or_default()
    })
}

/// Format a Slack notification message.
pub fn format_message(
    phase: &str,
    message: &str,
    feature: Option<&str>,
    pr_url: Option<&str>,
) -> String {
    let names = phase_names();
    let phase_name = names.get(phase).map(|s| s.as_str()).unwrap_or(phase);
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
#[allow(clippy::type_complexity)]
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
            "-X",
            "POST",
            SLACK_API_URL,
            "-H",
            &auth_header,
            "-H",
            "Content-Type: application/json; charset=utf-8",
            "-d",
            &payload_str,
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

            let ts = response.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            json!({"status": "ok", "ts": ts})
        }
    }
}

/// Run a curl-shaped subprocess with timeout via an injected child factory.
///
/// `child_factory` returns a spawned `Child` (with stdout/stderr piped) for
/// the supplied args. The seam exists so unit tests cover the success,
/// timeout-kill, and spawn-error branches without spawning real `curl`.
/// Production wraps this with a closure that calls `Command::new("curl")`.
pub fn run_curl_with_timeout_inner(
    args: &[&str],
    timeout_secs: u64,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> Result<(i32, String, String), String> {
    let mut child = child_factory(args).map_err(|e| format!("Failed to spawn curl: {}", e))?;

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

/// Run curl as a subprocess with timeout. Production binder over
/// [`run_curl_with_timeout_inner`].
fn run_curl_with_timeout(
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    run_curl_with_timeout_inner(args, timeout_secs, &|args| {
        Command::new("curl")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
}

/// Core notification logic with injectable config reader and poster.
///
/// `config_reader` returns the Slack credentials (or `None` when unconfigured).
/// `poster` accepts (bot_token, channel, text, thread_ts) and returns the JSON
/// response Value Slack produced. This split lets tests drive every branch
/// without touching env vars or spawning `curl`. Production `notify()` binds
/// the closures to `read_slack_config` and `post_message_inner +
/// run_curl_with_timeout`.
#[allow(clippy::type_complexity)]
pub fn notify_with_deps(
    args: &Args,
    config_reader: &dyn Fn() -> Option<SlackConfig>,
    poster: &dyn Fn(&str, &str, &str, Option<&str>) -> Value,
) -> Value {
    let config = match config_reader() {
        None => return json!({"status": "skipped", "reason": "no slack config"}),
        Some(c) => c,
    };

    let text = format_message(
        &args.phase,
        &args.message,
        args.feature.as_deref(),
        args.pr_url.as_deref(),
    );
    poster(
        &config.bot_token,
        &config.channel,
        &text,
        args.thread_ts.as_deref(),
    )
}

/// Core notification logic. Returns result Value.
pub fn notify(args: &Args) -> Value {
    notify_with_deps(args, &read_slack_config, &|bot, channel, text, tts| {
        post_message_inner(bot, channel, text, tts, &run_curl_with_timeout)
    })
}

/// CLI entry with injectable dependencies and writer.
///
/// Computes the notify result via `notify_with_deps` and writes it as JSON
/// followed by a newline to `writer`. Production `run` binds the closures
/// to `read_slack_config` + `post_message_inner(…, run_curl_with_timeout)`
/// and passes `std::io::stdout()` so the CLI prints a single JSON line.
#[allow(clippy::type_complexity)]
pub fn run_with_deps(
    args: Args,
    config_reader: &dyn Fn() -> Option<SlackConfig>,
    poster: &dyn Fn(&str, &str, &str, Option<&str>) -> Value,
    writer: &mut dyn std::io::Write,
) {
    let result = notify_with_deps(&args, config_reader, poster);
    let _ = writeln!(writer, "{}", result);
}

pub fn run(args: Args) {
    let mut stdout = std::io::stdout();
    run_with_deps(
        args,
        &read_slack_config,
        &|bot, channel, text, tts| {
            post_message_inner(bot, channel, text, tts, &run_curl_with_timeout)
        },
        &mut stdout,
    );
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
        let (code, stdout, stderr) =
            run_curl_with_timeout_inner(&["irrelevant"], 5, &factory).unwrap();
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
        assert!(
            err.to_lowercase().contains("timeout"),
            "expected timeout error, got {}",
            err
        );
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
        // Verify thread_ts was in the payload
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
        assert!(
            !*poster_called.borrow(),
            "poster must not be called when config is absent"
        );
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

    // --- run_with_deps ---

    #[test]
    fn run_with_deps_prints_notify_json() {
        let args = test_args("flow-start", "hi", None, None, None);
        let config_reader = || None;
        let poster = |_: &str, _: &str, _: &str, _: Option<&str>| -> Value {
            json!({"status": "ok", "ts": "9.9"})
        };
        let mut buf: Vec<u8> = Vec::new();
        run_with_deps(args, &config_reader, &poster, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        // When config_reader returns None the result is the skipped JSON.
        assert!(out.contains("\"status\":\"skipped\""));
        // writeln! appends a newline; the production path needs the newline
        // so `run` output is line-delimited for shell consumers.
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn run_with_deps_success_writes_ok_json() {
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
        let mut buf: Vec<u8> = Vec::new();
        run_with_deps(args, &config_reader, &poster, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\"status\":\"ok\""));
        assert!(out.contains("\"ts\":\"9.9\""));
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
        assert!(
            text.contains("Invoice Export"),
            "feature must flow into posted text: {}",
            *text
        );
        assert!(
            text.contains("https://github.com/org/repo/pull/42"),
            "pr_url must flow into posted text: {}",
            *text
        );
    }
}
