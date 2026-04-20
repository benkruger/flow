//! Check GitHub for newer FLOW releases.
//!
//! Reads plugin.json to determine installed version + repository URL,
//! calls `gh api repos/OWNER/REPO/releases/latest --jq .tag_name`, and
//! compares parsed version tuples. Honors `FLOW_PLUGIN_JSON` and
//! `FLOW_UPGRADE_TIMEOUT` env var overrides for testability.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::utils::plugin_root;

const DEFAULT_TIMEOUT_SECS: u64 = 10;

#[derive(Parser, Debug)]
#[command(name = "upgrade-check", about = "Check GitHub for newer FLOW releases")]
pub struct Args {}

/// Result of a single `gh` subprocess call — completed (with exit code
/// and output), timed out, or binary not found.
#[derive(Debug, Clone)]
pub enum GhResult {
    Ok {
        returncode: i32,
        stdout: String,
        stderr: String,
    },
    Timeout,
    NotFound,
}

/// Pure upgrade-check logic with injected gh command runner.
///
/// The `gh_cmd` closure receives `(owner_repo, timeout_secs)` and returns
/// a [`GhResult`]. Production code wraps [`run_gh_cmd`] which calls real
/// `gh`; tests provide mock closures returning pre-staged results.
pub fn upgrade_check_impl(
    plugin_json: &Path,
    timeout_secs: u64,
    gh_cmd: &mut dyn FnMut(&str, u64) -> GhResult,
) -> Value {
    // Step 1: read plugin.json
    let content = match std::fs::read_to_string(plugin_json) {
        Ok(c) => c,
        Err(_) => {
            return json!({
                "status": "unknown",
                "reason": "Could not read plugin.json",
            });
        }
    };
    let data: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            return json!({
                "status": "unknown",
                "reason": "Invalid plugin.json",
            });
        }
    };
    let installed = match data.get("version").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            return json!({
                "status": "unknown",
                "reason": "No version in plugin.json",
            });
        }
    };

    // Step 2: repository field → owner/repo
    let repository = data
        .get("repository")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let prefix = "https://github.com/";
    if !repository.starts_with(prefix) {
        return json!({
            "status": "unknown",
            "reason": "No GitHub repository URL in plugin.json",
        });
    }
    let owner_repo = repository[prefix.len()..].trim_end_matches('/').to_string();

    // Step 3: call gh
    let gh_result = gh_cmd(&owner_repo, timeout_secs);
    let (returncode, stdout) = match gh_result {
        GhResult::NotFound => {
            return json!({
                "status": "unknown",
                "reason": "gh CLI not found",
            });
        }
        GhResult::Timeout => {
            return json!({
                "status": "unknown",
                "reason": "GitHub API request timed out",
            });
        }
        GhResult::Ok {
            returncode, stdout, ..
        } => (returncode, stdout),
    };

    if returncode != 0 {
        return json!({
            "status": "unknown",
            "reason": format!("GitHub API request failed (exit {})", returncode),
        });
    }

    let tag = stdout.trim().to_string();
    if tag.is_empty() {
        return json!({
            "status": "unknown",
            "reason": "No releases found",
        });
    }

    // Step 4: parse versions and compare
    let latest = tag.trim_start_matches('v').to_string();
    let latest_tuple = match parse_version(&latest) {
        Some(t) => t,
        None => {
            return json!({
                "status": "unknown",
                "reason": format!("Could not parse version: {}", tag),
            });
        }
    };
    let installed_tuple = match parse_version(&installed) {
        Some(t) => t,
        None => {
            // The error message must cite the locally-stored installed
            // version (not the remote tag) so the user sees which value
            // is malformed and where to fix it (their plugin.json, not
            // GitHub). The matching test below trip-wires regressions
            // that swap these two values.
            return json!({
                "status": "unknown",
                "reason": format!("Could not parse version: {}", installed),
            });
        }
    };

    if latest_tuple > installed_tuple {
        json!({
            "status": "upgrade_available",
            "installed": installed,
            "latest": latest,
        })
    } else {
        json!({"status": "current", "installed": installed})
    }
}

/// Parse `"1.2.3"` into `(1, 2, 3)`. Requires exactly 3 dotted
/// integers; any deviation (extra parts, non-numeric segment,
/// missing parts) returns `None` so callers can decide how to handle
/// a malformed version string.
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let a = parts[0].parse::<u32>().ok()?;
    let b = parts[1].parse::<u32>().ok()?;
    let c = parts[2].parse::<u32>().ok()?;
    Some((a, b, c))
}

/// Real `gh` subprocess runner with polling-based timeout and thread-drain.
///
/// Uses the thread-drain pattern to prevent pipe buffer deadlock: take
/// stdout/stderr handles before the poll
/// loop, drain them in spawned reader threads, poll `try_wait()` for exit
/// status, then join the readers.
///
/// Any spawn failure (NotFound, PermissionDenied, etc.) collapses to
/// `GhResult::NotFound` so the caller treats every spawn-failure
/// shape uniformly as "gh is unavailable" instead of crashing on
/// unexpected error variants.
/// Spawn a pre-built `Command` with piped stdout/stderr, drain the
/// pipes in reader threads to avoid buffer deadlock, and poll for exit
/// until `timeout_secs` elapses. Returns `GhResult::Timeout` on
/// deadline, `GhResult::NotFound` when spawn itself fails.
pub fn run_gh_with_command(mut cmd: Command, timeout_secs: u64) -> GhResult {
    use std::io::Read;

    let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return GhResult::NotFound,
    };

    // stdout/stderr are guaranteed Some because we passed
    // `Stdio::piped()` above.
    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");
    let stdout_reader = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_handle.read_to_string(&mut buf);
        buf
    });
    let stderr_reader = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_handle.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    // try_wait on an owned Child is infallible in practice.
    let status = loop {
        match child
            .try_wait()
            .expect("try_wait on owned child is infallible")
        {
            Some(s) => break s,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return GhResult::Timeout;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    };

    // The reader closures cannot panic — read_to_string on a pipe
    // returns Result which they discard. `.expect` on the Err arm
    // is genuinely unreachable per
    // `.claude/rules/testability-means-simplicity.md`.
    let stdout = stdout_reader
        .join()
        .expect("stdout_reader thread cannot panic");
    let stderr = stderr_reader
        .join()
        .expect("stderr_reader thread cannot panic");

    GhResult::Ok {
        returncode: status.code().unwrap_or(-1),
        stdout,
        stderr,
    }
}

/// Real `gh` subprocess runner. Builds `gh api repos/<owner>/<repo>/releases/latest`
/// and delegates to [`run_gh_with_command`].
pub fn run_gh_cmd(owner_repo: &str, timeout_secs: u64) -> GhResult {
    let api_path = format!("repos/{}/releases/latest", owner_repo);
    let mut cmd = Command::new("gh");
    cmd.args(["api", &api_path, "--jq", ".tag_name"]);
    run_gh_with_command(cmd, timeout_secs)
}

/// Pure helper: resolve plugin.json path given an explicit env override
/// and a pre-resolved plugin_root value. Tests call this directly to
/// drive every branch (env Some, env None + plugin_root Some, env None
/// + plugin_root None).
pub fn resolve_plugin_json_path_with_root(
    env_override: Option<String>,
    plugin_root_value: Option<PathBuf>,
) -> PathBuf {
    if let Some(p) = env_override {
        return PathBuf::from(p);
    }
    match plugin_root_value {
        Some(r) => r.join(".claude-plugin").join("plugin.json"),
        None => PathBuf::from(".claude-plugin/plugin.json"),
    }
}

/// Resolve the plugin.json path from optional env var + fallback.
/// Extracted from `run()` for testability. Thin wrapper around
/// `resolve_plugin_json_path_with_root` that supplies the real
/// `plugin_root()`.
pub fn resolve_plugin_json_path(env_override: Option<String>) -> PathBuf {
    resolve_plugin_json_path_with_root(env_override, plugin_root())
}

/// Resolve the timeout from an optional env var with default fallback.
pub fn resolve_timeout(env_override: Option<String>) -> u64 {
    env_override
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

/// CLI entry point. Reads `FLOW_PLUGIN_JSON` and `FLOW_UPGRADE_TIMEOUT`
/// env vars, resolves the plugin.json path, and calls
/// [`upgrade_check_impl`] with a real gh closure. Always exits 0 —
/// "unknown" is a normal status, not an error.
pub fn run(_args: Args) {
    let plugin_json_path = resolve_plugin_json_path(env::var("FLOW_PLUGIN_JSON").ok());
    let timeout = resolve_timeout(env::var("FLOW_UPGRADE_TIMEOUT").ok());

    let mut gh = |owner_repo: &str, timeout_secs: u64| run_gh_cmd(owner_repo, timeout_secs);
    let result = upgrade_check_impl(&plugin_json_path, timeout, &mut gh);
    println!("{}", serde_json::to_string(&result).unwrap());
}
