//! Port of lib/upgrade-check.py — check GitHub for newer FLOW releases.
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

/// Result of a single `gh` subprocess call. Mirrors Python
/// `subprocess.CompletedProcess` + `TimeoutExpired` + `FileNotFoundError`.
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
            return json!({
                "status": "unknown",
                "reason": format!("Could not parse version: {}", tag),
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

/// Parse `"1.2.3"` into `(1, 2, 3)`. Requires exactly 3 dotted integers —
/// returns `None` on any parse error to match Python's strict tuple parse.
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
/// Uses the thread-drain pattern from `.claude/rules/rust-port-parity.md`
/// Subprocess Timeout Parity: take stdout/stderr handles before the poll
/// loop, drain them in spawned reader threads, poll `try_wait()` for exit
/// status, then join the readers. Compliant reference: see
/// `src/analyze_issues.rs` lines 472-518.
///
/// Any spawn failure (NotFound, PermissionDenied, etc.) is mapped to
/// `GhResult::NotFound` — a deliberate improvement over Python's
/// `FileNotFoundError`-only handler which would panic on other errors.
fn run_gh_cmd(owner_repo: &str, timeout_secs: u64) -> GhResult {
    use std::io::Read;

    let api_path = format!("repos/{}/releases/latest", owner_repo);
    let mut child = match Command::new("gh")
        .args(["api", &api_path, "--jq", ".tag_name"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return GhResult::NotFound,
    };

    // Drain stdout/stderr in threads to prevent pipe buffer deadlock.
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_handle {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_reader = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr_handle {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Join readers even on timeout so they do not leak.
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return GhResult::Timeout;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return GhResult::Ok {
                    returncode: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    GhResult::Ok {
        returncode: status.and_then(|s| s.code()).unwrap_or(-1),
        stdout,
        stderr,
    }
}

/// CLI entry point. Reads `FLOW_PLUGIN_JSON` and `FLOW_UPGRADE_TIMEOUT`
/// env vars, resolves the plugin.json path, and calls
/// [`upgrade_check_impl`] with a real gh closure. Always exits 0 —
/// "unknown" is a normal status, not an error.
pub fn run(_args: Args) {
    let plugin_json_path = match env::var("FLOW_PLUGIN_JSON").ok() {
        Some(p) => PathBuf::from(p),
        None => plugin_root()
            .map(|r| r.join(".claude-plugin").join("plugin.json"))
            .unwrap_or_else(|| PathBuf::from(".claude-plugin/plugin.json")),
    };
    let timeout = env::var("FLOW_UPGRADE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);

    let mut gh = |owner_repo: &str, timeout_secs: u64| run_gh_cmd(owner_repo, timeout_secs);
    let result = upgrade_check_impl(&plugin_json_path, timeout, &mut gh);
    println!("{}", serde_json::to_string(&result).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    fn write_plugin_json(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("plugin.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn current_version() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
            returncode: 0,
            stdout: "v1.0.0".to_string(),
            stderr: String::new(),
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result, json!({"status": "current", "installed": "1.0.0"}));
    }

    #[test]
    fn upgrade_available() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
            returncode: 0,
            stdout: "v1.1.0".to_string(),
            stderr: String::new(),
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(
            result,
            json!({
                "status": "upgrade_available",
                "installed": "1.0.0",
                "latest": "1.1.0",
            })
        );
    }

    #[test]
    fn gh_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::NotFound;
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn network_failure() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
            returncode: 1,
            stdout: String::new(),
            stderr: "connection refused".to_string(),
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"].as_str().unwrap().contains("failed"));
    }

    #[test]
    fn no_releases() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
            returncode: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"].as_str().unwrap().contains("No releases"));
    }

    #[test]
    fn malformed_tag() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
            returncode: 0,
            stdout: "not-a-version".to_string(),
            stderr: String::new(),
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("parse"));
    }

    #[test]
    fn no_repository_url() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(dir.path(), r#"{"version":"1.0.0"}"#);
        let mut gh = |_owner_repo: &str, _t: u64| -> GhResult {
            panic!("gh should not be called when repository is missing");
        };
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("repository"));
    }

    #[test]
    fn timeout() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = write_plugin_json(
            dir.path(),
            r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
        );
        let mut gh = |_owner_repo: &str, _t: u64| GhResult::Timeout;
        let result = upgrade_check_impl(&plugin, 10, &mut gh);
        assert_eq!(result["status"], "unknown");
        assert!(result["reason"].as_str().unwrap().contains("timed out"));
    }
}
