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
//! The module exposes a layered architecture so unit tests can drive
//! every branch without env-var mutation or real `curl` subprocesses:
//!
//! - [`notify_with_deps`] — dependency-injected core. Accepts a
//!   `config_reader` closure returning `Option<SlackConfig>` and a
//!   `poster` closure returning the Slack JSON response. Fully testable.
//! - [`notify`] — production binder that wires `notify_with_deps` to
//!   [`read_slack_config`] (env-var reader) and [`post_message_inner`]
//!   bound to [`run_curl_with_timeout`] (real curl subprocess).
//! - [`read_slack_config_with_env`] — env-var reader parameterized over
//!   `token_reader` and `channel_reader` closures so tests can drive
//!   present/absent/empty without process-wide env mutation.
//! - [`run_curl_with_timeout_inner`] — curl subprocess wrapper
//!   parameterized over a `child_factory` closure so tests can drive
//!   spawn failure, timeout, and stderr capture without real `curl`.
//! - [`run_impl_main`] — main-arm dispatcher accepting injected
//!   `config_reader` and `poster` closures, returning `(Value, i32)`.

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
pub fn run_curl_with_timeout(
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

/// Main-arm dispatcher: compute the notify result and pair it with an
/// exit code. Always returns `(value, 0)` — failure modes surface as
/// `status: "error"` inside the Value, never via shell exit code.
#[allow(clippy::type_complexity)]
pub fn run_impl_main(
    args: Args,
    config_reader: &dyn Fn() -> Option<SlackConfig>,
    poster: &dyn Fn(&str, &str, &str, Option<&str>) -> Value,
) -> (Value, i32) {
    (notify_with_deps(&args, config_reader, poster), 0)
}
