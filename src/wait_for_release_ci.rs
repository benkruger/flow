//! `bin/flow wait-for-release-ci` — block until the latest GitHub
//! Actions run on the integration branch reaches a terminal conclusion.
//!
//! flow-release Step 2 verifies CI on the integration branch before
//! bumping the version. When the latest run is still in progress, the
//! skill needs to wait rather than stop. This subcommand polls
//! `gh run list` with a real `thread::sleep` retry loop — modeled on the
//! loop shape of [`crate::commands::start_lock::acquire_with_wait`]
//! (timeout/interval parameters, no closure seam) — until the run for the
//! current HEAD reaches a terminal conclusion, then reports it.
//!
//! Output (JSON to stdout, always exit 0 — callers branch on `status`):
//!   - `{"status":"ready","conclusion":"<c>"}` — the run for HEAD
//!     finished (success / failure / cancelled / …). The caller branches
//!     on `conclusion`.
//!   - `{"status":"still_pending","waited_seconds":<n>}` — the run never
//!     reached a terminal conclusion before the timeout cap.
//!   - `{"status":"error","message":"..."}` — gh/git failure, no runs
//!     found, or the latest run is for a different commit (headSha
//!     mismatch, i.e. CI has not run on the latest commit).
//!
//! The `gh` and `git` subprocesses are fixture-controllable from
//! integration tests (PATH-shimmed `git`/`gh`), so this module exposes
//! only [`wait_for_release_ci`] / [`run_impl_main`] and is covered
//! entirely through `tests/wait_for_release_ci.rs` subprocess tests per
//! `.claude/rules/test-placement.md`.

use std::cmp::min;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(
    name = "wait-for-release-ci",
    about = "Wait for the latest integration-branch CI run to finish"
)]
pub struct Args {
    /// Integration branch whose latest GitHub Actions run to poll.
    #[arg(long)]
    pub base: String,

    /// Max seconds to wait before reporting still_pending (default 480 = 8 min).
    #[arg(long, default_value = "480")]
    pub timeout: u64,

    /// Seconds between poll attempts (default 15).
    #[arg(long, default_value = "15")]
    pub interval: u64,
}

/// Outcome of one poll tick.
enum Tick {
    /// The run for HEAD reached a terminal conclusion.
    Ready(String),
    /// The run for HEAD exists but has not concluded yet.
    Pending,
    /// gh/git failure, no runs found, or headSha mismatch.
    Error(String),
}

/// Read the current HEAD commit SHA via `git rev-parse HEAD` in `cwd`.
/// Returns `None` when the git binary cannot be spawned, the command
/// exits non-zero (cwd is not a git repository), OR the trimmed stdout
/// is not a non-empty all-hex SHA. The last guard fails closed against a
/// broken or edge git that exits 0 with empty or non-hex output: an
/// empty HEAD would otherwise let `classify`'s headSha-mismatch gate
/// compare "" against a run with a missing or empty headSha, pass, and
/// report a release-ready run on an unverified commit.
fn head_sha(cwd: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(sha)
}

/// Classify `gh run list --json conclusion,headSha` stdout against the
/// current HEAD. Pure over the JSON string so every parse branch is
/// reachable through [`wait_for_release_ci`]'s poll loop:
///   - unparseable JSON or non-array → `Error`
///   - empty array → `Error` (no runs on the branch)
///   - first run's headSha != `head` → `Error` (CI not run on latest commit)
///   - first run's conclusion null/missing → `Pending`
///   - first run's conclusion present → `Ready(conclusion)`
fn classify(stdout: &str, head: &str, base: &str) -> Tick {
    let runs: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => {
            return Tick::Error(format!(
                "could not parse `gh run list` output for branch {}",
                base
            ))
        }
    };
    let first = match runs.as_array().and_then(|a| a.first()) {
        Some(r) => r,
        None => return Tick::Error(format!("no CI runs found on branch {}", base)),
    };
    let run_sha = first.get("headSha").and_then(|v| v.as_str()).unwrap_or("");
    if run_sha != head {
        return Tick::Error(format!(
            "latest CI run on branch {} is for a different commit (headSha {} != HEAD {}); CI has not run on the latest commit",
            base, run_sha, head
        ));
    }
    match first.get("conclusion").and_then(|v| v.as_str()) {
        Some(c) => Tick::Ready(c.to_string()),
        None => Tick::Pending,
    }
}

/// Run one poll tick: read HEAD, run `gh run list`, classify.
fn poll(base: &str, cwd: &Path) -> Tick {
    let head = match head_sha(cwd) {
        Some(h) => h,
        None => return Tick::Error("git rev-parse HEAD failed".to_string()),
    };
    let output = Command::new("gh")
        .args([
            "run",
            "list",
            "--branch",
            base,
            "--limit",
            "1",
            "--json",
            "conclusion,headSha",
        ])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => classify(&String::from_utf8_lossy(&o.stdout), &head, base),
        Ok(o) => Tick::Error(format!(
            "`gh run list` failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Tick::Error(format!("failed to spawn gh: {}", e)),
    }
}

/// Poll `gh run list` until the latest run on `base` reaches a terminal
/// conclusion, with a real `thread::sleep` retry loop bounded by
/// `timeout`. Mirrors the loop shape of
/// [`crate::commands::start_lock::acquire_with_wait`]: check first, then
/// sleep `min(interval, remaining)` between ticks so the final sleep
/// never overshoots the cap.
pub fn wait_for_release_ci(base: &str, cwd: &Path, timeout: u64, interval: u64) -> Value {
    if base.trim().is_empty() {
        return json!({
            "status": "error",
            "message": "wait-for-release-ci requires a non-empty --base"
        });
    }
    let start = Instant::now();
    loop {
        match poll(base, cwd) {
            Tick::Ready(conclusion) => return json!({"status": "ready", "conclusion": conclusion}),
            Tick::Error(msg) => return json!({"status": "error", "message": msg}),
            Tick::Pending => {}
        }
        let elapsed = start.elapsed().as_secs();
        if elapsed >= timeout {
            return json!({"status": "still_pending", "waited_seconds": elapsed as i64});
        }
        let remaining = timeout - elapsed;
        thread::sleep(Duration::from_secs(min(interval, remaining)));
    }
}

/// Main-arm dispatcher. Always returns exit code 0 — callers branch on
/// the JSON `status` field (ready / still_pending / error) per
/// `.claude/rules/rust-patterns.md` "Exit code convention for business
/// errors".
pub fn run_impl_main(args: &Args, cwd: &Path) -> (Value, i32) {
    (
        wait_for_release_ci(&args.base, cwd, args.timeout, args.interval),
        0,
    )
}
