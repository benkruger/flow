//! Shared utilities — timestamps, branch-name normalization, GitHub
//! helpers, permission-glob regex conversion, tab color, and
//! tolerant JSON integer reading.
//!
//! Tests live at `tests/utils.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::America::Los_Angeles;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::flow_paths::FlowStatesDir;

// --- SetupError + run_cmd ---

/// Error type for start-phase subprocess operations (start-workspace, auto-close-parent).
/// Captures a step identifier for tracing which operation failed.
#[derive(Debug)]
pub struct SetupError {
    pub step: String,
    pub message: String,
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.step, self.message)
    }
}

/// Given a completed child's exit status plus captured stdout/stderr,
/// produce the run_cmd result contract: Ok tuple on success, Err
/// with the best-available error message on failure.
///
/// Empty stderr falls back to stdout so commands that report failures
/// on stdout (rare) still surface a meaningful message.
pub fn classify_output(
    status: std::process::ExitStatus,
    stdout_bytes: &[u8],
    stderr_bytes: &[u8],
    step_name: &str,
) -> Result<(String, String), SetupError> {
    let stdout = String::from_utf8_lossy(stdout_bytes).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr_bytes).trim().to_string();
    if status.success() {
        Ok((stdout, stderr))
    } else {
        Err(SetupError {
            step: step_name.to_string(),
            message: if stderr.is_empty() { stdout } else { stderr },
        })
    }
}

/// Run a shell command with optional timeout, returning (stdout, stderr).
/// Used by start-workspace (worktree/PR creation) and auto-close-parent.
///
/// The no-timeout path uses `Command::output()` — a single call that
/// spawns, waits, and captures both streams. The timeout path polls
/// `try_wait()` with a 50ms interval; on timeout the child is killed
/// and reaped.
pub fn run_cmd(
    args: &[&str],
    cwd: &Path,
    step_name: &str,
    timeout: Option<Duration>,
) -> Result<(String, String), SetupError> {
    if let Some(dur) = timeout {
        let mut child = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| SetupError {
                step: step_name.to_string(),
                message: format!("Failed to spawn: {}", e),
            })?;
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        let status = loop {
            // try_wait can only fail for genuinely pathological reasons
            // (child adopted by another process, OS inconsistency).
            // Treat any non-Ok(Some) result as "still running" so the
            // timeout deadline ultimately decides the outcome.
            if let Ok(Some(s)) = child.try_wait() {
                break s;
            }
            if start.elapsed() >= dur {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SetupError {
                    step: step_name.to_string(),
                    message: format!("Timed out after {}s", dur.as_secs()),
                });
            }
            std::thread::sleep(poll_interval.min(dur - start.elapsed()));
        };
        // Child already exited — drain the piped stdout/stderr.
        // Both streams were set to `Stdio::piped()` above, so `take()`
        // is guaranteed to return `Some`. Any read error is a truly
        // pathological OS state; discard it and surface whatever
        // bytes were buffered.
        let mut stdout_pipe = child.stdout.take().expect("stdout piped");
        let mut stderr_pipe = child.stderr.take().expect("stderr piped");
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut stdout_buf);
        let _ = stderr_pipe.read_to_end(&mut stderr_buf);
        classify_output(status, &stdout_buf, &stderr_buf, step_name)
    } else {
        let output = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .output()
            .map_err(|e| SetupError {
                step: step_name.to_string(),
                message: format!("Failed to spawn: {}", e),
            })?;
        classify_output(output.status, &output.stdout, &output.stderr, step_name)
    }
}

// --- Version reading ---

/// Read plugin version from a specific plugin.json path.
///
/// Returns "?" on any error (missing file, bad JSON, no version key).
pub fn read_version_from(path: &Path) -> String {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return "?".to_string(),
    };
    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return "?".to_string(),
    };
    match data.get("version").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => "?".to_string(),
    }
}

/// Read plugin version from `.claude-plugin/plugin.json`.
///
/// Resolution order:
///   1. If `CLAUDE_PLUGIN_ROOT` is set and its `.claude-plugin/plugin.json`
///      exists, use that. Otherwise fall through.
///   2. Walk up 3 levels from the binary: `flow-rs` → `{debug|release}`
///      → `target` → plugin root. Read `.claude-plugin/plugin.json` there.
///
/// The env var check in (1) was added because cargo-llvm-cov builds the
/// binary at `target/llvm-cov-target/debug/flow-rs` — one extra directory
/// beyond what the 3-level walk assumes. Tests that need the version
/// under coverage set `CLAUDE_PLUGIN_ROOT` explicitly and take path (1).
/// When the env var is not set, production behavior is unchanged: the
/// original 3-level walk.
pub fn read_version() -> String {
    read_version_with(
        std::env::var("CLAUDE_PLUGIN_ROOT").ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    )
}

/// Testable seam for `read_version`. Accepts the env var value and the
/// current-exe path as parameters so every branch is reachable from
/// unit tests without mutating process env.
pub fn read_version_with(claude_plugin_root: Option<&str>, current_exe: Option<&Path>) -> String {
    if let Some(root) = claude_plugin_root {
        let path = std::path::PathBuf::from(root)
            .join(".claude-plugin")
            .join("plugin.json");
        if path.exists() {
            return read_version_from(&path);
        }
    }
    let exe = match current_exe {
        Some(p) => p,
        None => return "?".to_string(),
    };
    // Walk up to 5 levels from the binary looking for the plugin
    // root (identified by `.claude-plugin/plugin.json`). 5 levels
    // covers both the release layout (`<plugin>/target/release/flow-rs`
    // → 3 up) and the cargo-llvm-cov test layout
    // (`<plugin>/target/llvm-cov-target/debug/deps/<test>-HASH`
    // → 5 up). Fixed levels were brittle; a bounded walk matches
    // both.
    let mut dir = match exe.parent() {
        Some(p) => p,
        None => return "?".to_string(),
    };
    for _ in 0..5 {
        let plugin_json = dir.join(".claude-plugin").join("plugin.json");
        if plugin_json.exists() {
            return read_version_from(&plugin_json);
        }
        dir = match dir.parent() {
            Some(p) => p,
            None => return "?".to_string(),
        };
    }
    "?".to_string()
}

// --- Plugin root ---

/// Find the plugin root directory (where flow-phases.json lives).
///
/// Checks CLAUDE_PLUGIN_ROOT env var first, then walks up from the
/// current executable's location.
pub fn plugin_root() -> Option<std::path::PathBuf> {
    plugin_root_with(
        std::env::var("CLAUDE_PLUGIN_ROOT").ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    )
}

/// Testable seam for `plugin_root`. Accepts the env var value and the
/// current-exe path as parameters so tests can drive every branch.
pub fn plugin_root_with(
    claude_plugin_root: Option<&str>,
    current_exe: Option<&Path>,
) -> Option<std::path::PathBuf> {
    if let Some(root) = claude_plugin_root {
        let path = std::path::PathBuf::from(root);
        if path.join("flow-phases.json").exists() {
            return Some(path);
        }
    }
    let exe = current_exe?;
    let mut dir = exe.parent()?;
    for _ in 0..5 {
        if dir.join("flow-phases.json").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
    None
}

/// Locate `bin/flow` via `current_exe` traversal, falling back to `"bin/flow"`.
///
/// The binary lives at `<repo>/target/{release|debug}/flow-rs`, so three
/// `.parent()` calls reach the repo root. Hoisted from four identical private
/// copies to eliminate duplication. Shared by complete_preflight,
/// complete_merge, complete_post_merge, and complete_fast.
pub fn bin_flow_path() -> String {
    bin_flow_path_with(
        std::env::var("FLOW_BIN_PATH").ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    )
}

/// Testable seam for `bin_flow_path`. Accepts the env override and the
/// current-exe path as parameters so every branch is reachable from
/// unit tests without mutating process env.
pub fn bin_flow_path_with(flow_bin_path: Option<&str>, current_exe: Option<&Path>) -> String {
    // Env-var override for subprocess tests that need to stub
    // `bin/flow` at a test-isolated path. Production callers never
    // set this variable; it is read only when present.
    if let Some(override_path) = flow_bin_path {
        if !override_path.is_empty() {
            return override_path.to_string();
        }
    }
    current_exe
        .and_then(|p| p.parent()?.parent()?.parent().map(|d| d.to_path_buf()))
        .map(|d: std::path::PathBuf| d.join("bin").join("flow"))
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "bin/flow".to_string())
}

// --- Tab color constants ---

/// Terminal tab colors (firebrick, teal, indigo, dark goldenrod, dark green,
/// maroon, steel blue, saddle brown, dark slate blue, dark cyan, sienna, midnight blue).
pub const TAB_COLORS: [(u8, u8, u8); 12] = [
    (178, 34, 34),  // firebrick
    (0, 128, 128),  // teal
    (75, 0, 130),   // indigo
    (184, 134, 11), // dark goldenrod
    (0, 100, 0),    // dark green
    (128, 0, 0),    // maroon
    (70, 130, 180), // steel blue
    (139, 69, 19),  // saddle brown
    (72, 61, 139),  // dark slate blue
    (0, 139, 139),  // dark cyan
    (160, 82, 45),  // sienna
    (25, 25, 112),  // midnight blue
];

/// Pinned colors for specific repos.
pub fn pinned_color(repo: &str) -> Option<(u8, u8, u8)> {
    match repo {
        "HipaaHealth/mono-repo" => Some((50, 120, 220)),
        "benkruger/salted-kitchen" => Some((220, 130, 20)),
        "benkruger/flow" => Some((40, 180, 70)),
        _ => None,
    }
}

// --- Timestamp functions ---

/// Return current Pacific Time timestamp in ISO 8601 format.
pub fn now() -> String {
    let utc_now = Utc::now();
    let pacific = utc_now.with_timezone(&Los_Angeles);
    pacific.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Format seconds into human-readable time.
///
/// Returns "Xh Ym" if >= 3600, "Xm" if >= 60, "<1m" if < 60.
/// Returns "?" for negative or invalid values.
pub fn format_time(seconds: i64) -> String {
    if seconds < 0 {
        return "?".to_string();
    }
    if seconds >= 3600 {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        return format!("{}h {}m", hours, minutes);
    }
    if seconds >= 60 {
        let minutes = seconds / 60;
        return format!("{}m", minutes);
    }
    "<1m".to_string()
}

/// Format an integer token count as a compact string: `1.2K`, `3.4M`,
/// or the raw integer when below 1000. Stable formatting so test
/// assertions can pin specific values. Used by `format_complete_summary`,
/// `format_status`, and `tui` to render Token Cost / Tokens panels.
pub fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Calculate elapsed seconds from an ISO timestamp to now (or a given time).
///
/// Returns 0 if started_at is None or empty. Never returns negative.
pub fn elapsed_since(started_at: Option<&str>, now_override: Option<DateTime<FixedOffset>>) -> i64 {
    let started = match started_at {
        Some(s) if !s.is_empty() => s,
        _ => return 0,
    };

    let start = match DateTime::parse_from_rfc3339(started) {
        Ok(dt) => dt,
        Err(_) => return 0,
    };

    let now_dt = match now_override {
        Some(dt) => dt,
        None => {
            let utc_now = Utc::now();
            let pacific = utc_now.with_timezone(&Los_Angeles);
            pacific.fixed_offset()
        }
    };

    let elapsed = (now_dt - start).num_seconds();
    if elapsed < 0 {
        0
    } else {
        elapsed
    }
}

// --- Branch and feature name functions ---

/// Convert feature words to a hyphenated branch name, max 32 chars.
pub fn branch_name(feature_words: &str) -> String {
    let re = Regex::new(r"[^a-zA-Z0-9\s\-]").unwrap();
    let sanitized = re.replace_all(feature_words, "");
    let name: String = sanitized
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("-");

    // Fallback for feature descriptions with no alphanumeric characters
    // (empty input, pure punctuation, or all-unicode). Returns a safe
    // placeholder rather than an empty string that would break downstream
    // worktree creation and git operations.
    if name.is_empty() {
        return "unnamed".to_string();
    }

    if name.chars().count() <= 32 {
        return name;
    }

    let truncated: String = name.chars().take(33).collect();
    if let Some(pos) = truncated.rfind('-') {
        if pos > 0 {
            return truncated[..pos].to_string();
        }
    }
    name.chars().take(32).collect()
}

/// Derive the human-readable feature name from a branch name.
///
/// Title-cases each hyphen-separated word.
pub fn derive_feature(branch: &str) -> String {
    branch
        .split('-')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.collect::<String>())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Derive the worktree path from a branch name.
pub fn derive_worktree(branch: &str) -> String {
    format!(".worktrees/{}", branch)
}

// --- Issue and prompt functions ---

/// Extract unique issue numbers from #N patterns and GitHub URLs in a prompt string.
pub fn extract_issue_numbers(prompt: &str) -> Vec<i64> {
    let hash_re = Regex::new(r"#(\d+)").unwrap();
    let url_re = Regex::new(r"/issues/(\d+)").unwrap();

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for cap in hash_re
        .captures_iter(prompt)
        .chain(url_re.captures_iter(prompt))
    {
        if let Ok(num) = cap[1].parse::<i64>() {
            if seen.insert(num) {
                result.push(num);
            }
        }
    }
    result
}

/// Info about a duplicate flow targeting the same issue.
#[derive(Debug)]
pub struct DuplicateInfo {
    pub branch: String,
    pub phase: String,
    pub pr_url: String,
}

/// Issue metadata fetched from GitHub — used by init-state to derive branch
/// names and to check the Flow In-Progress label guard for issue #887.
///
/// The `labels` field uses `deserialize_null_to_default` (via `deserialize_with`)
/// to coerce both absent keys AND explicit `null` values to an empty vec. This
/// is load-bearing defensive handling: a gh/jq shape that produces `"labels": null`
/// would otherwise fail deserialization and surface as a misleading fetch error,
/// hiding the real problem from users running into the label guard.
#[derive(Debug, Deserialize)]
pub struct IssueInfo {
    pub title: String,
    #[serde(default, deserialize_with = "deserialize_null_to_default")]
    pub labels: Vec<String>,
}

/// Deserialize helper that treats explicit JSON `null` as `T::default()`.
///
/// Combined with `#[serde(default)]` on the same field, this handles all three
/// shapes a JSON key can take: present with a value, present with null, or
/// absent entirely. See `IssueInfo::labels` for the primary consumer.
fn deserialize_null_to_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::<T>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Fetch issue title and labels from GitHub in a single `gh` call.
/// Returns None on fetch failure, parse failure, or empty title.
/// Uses a 10-second timeout — short enough that an unreachable GitHub
/// does not block flow-start indefinitely, long enough to tolerate
/// normal API jitter.
pub fn fetch_issue_info(issue_number: i64) -> Option<IssueInfo> {
    fetch_issue_info_with_cmd("gh", issue_number)
}

/// Testable seam for `fetch_issue_info` that makes the command binary
/// injectable. Tests drive the success path by passing a command
/// known to exit 0 (like `echo`) and the failure path by passing a
/// nonexistent binary.
pub fn fetch_issue_info_with_cmd(cmd: &str, issue_number: i64) -> Option<IssueInfo> {
    let (stdout, _) = run_cmd(
        &[
            cmd,
            "issue",
            "view",
            &issue_number.to_string(),
            "--json",
            "title,labels",
            "--jq",
            "{title, labels: [.labels[].name]}",
        ],
        std::path::Path::new("."),
        "fetch_issue_info",
        Some(Duration::from_secs(10)),
    )
    .ok()?;
    parse_issue_info(&stdout)
}

/// Parse the `gh issue view` JSON stdout into an `IssueInfo`.
///
/// Returns `None` when the JSON is malformed OR when the title is
/// empty. Extracted as a pub seam so tests can cover both arms
/// without spawning a `gh` subprocess.
pub fn parse_issue_info(stdout: &str) -> Option<IssueInfo> {
    let info: IssueInfo = serde_json::from_str(stdout.trim()).ok()?;
    if info.title.is_empty() {
        None
    } else {
        Some(info)
    }
}

/// Check if an existing flow already targets the same issue numbers.
pub fn check_duplicate_issue(
    project_root: &Path,
    issue_numbers: &[i64],
    self_branch: &str,
) -> Option<DuplicateInfo> {
    if issue_numbers.is_empty() {
        return None;
    }
    let state_dir = FlowStatesDir::new(project_root).path().to_path_buf();
    if !state_dir.is_dir() {
        return None;
    }
    let target_issues: std::collections::HashSet<i64> = issue_numbers.iter().copied().collect();

    let mut entries: Vec<_> = std::fs::read_dir(&state_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.ends_with(".json") {
            continue;
        }
        if name_str.ends_with("-phases.json") {
            continue;
        }
        let stem = name_str.trim_end_matches(".json");
        if stem == self_branch {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let state: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Skip completed flows — their state files are normally deleted by
        // cleanup, but leftovers should not block new flows for the same issue.
        let is_completed = state
            .get("phases")
            .and_then(|p| p.get("flow-complete"))
            .and_then(|fc| fc.get("status"))
            .and_then(|s| s.as_str())
            == Some("complete");
        if is_completed {
            continue;
        }

        let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let existing_issues: std::collections::HashSet<i64> =
            extract_issue_numbers(prompt).into_iter().collect();

        if !existing_issues.is_disjoint(&target_issues) {
            return Some(DuplicateInfo {
                branch: state
                    .get("branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or(stem)
                    .to_string(),
                phase: state
                    .get("current_phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                pr_url: state
                    .get("pr_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    None
}

/// Extract '#N' from a GitHub issue URL, falling back to the full URL.
pub fn short_issue_ref(url: &str) -> String {
    let re = Regex::new(r"/issues/(\d+)$").unwrap();
    match re.captures(url) {
        Some(cap) => format!("#{}", &cap[1]),
        None => url.to_string(),
    }
}

/// Read prompt text from a file and delete the file.
///
/// Returns Ok(content) on success, Err on failure.
/// The file is always deleted after reading, even if empty.
pub fn read_prompt_file(path: &Path) -> Result<String, io::Error> {
    let content = fs::read_to_string(path)?;
    let _ = fs::remove_file(path);
    Ok(content)
}

// --- Git conflict parsing ---

/// Parse git status --porcelain output and return conflict file paths.
///
/// Detects UU, AA, DD, and any status containing 'U' as conflict markers.
pub fn parse_conflict_files(porcelain_output: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in porcelain_output.lines() {
        if line.is_empty() {
            continue;
        }
        let xy = &line[..2.min(line.len())];
        if (xy.contains('U') || xy == "DD" || xy == "AA") && line.len() > 3 {
            files.push(line[3..].trim().to_string());
        }
    }
    files
}

// --- Permission regex ---

/// Convert a `Type(pattern)` permission to a compiled regex.
///
/// Extracts the inner pattern from any permission type and converts
/// glob wildcards to regex:
///
/// `Bash(git push)` → `^git push$`
/// `Bash(git push *)` → `^git push .*$`
/// `Agent(*)` → `^.*$`
/// `Read(~/.claude/rules/*)` → `^~/\.claude/rules/.*$`
///
/// Returns `None` for entries that don't match the `Type(pattern)` format.
///
/// Note: `src/hooks/mod.rs` has a Bash-only variant used by
/// `build_permission_regexes` for PreToolUse hook validation.
/// That version is intentionally restricted to `Bash(...)` entries
/// because the hook only validates Bash tool commands.
pub fn permission_to_regex(perm: &str) -> Option<Regex> {
    let outer_re = Regex::new(r"^\w+\((.+)\)$").unwrap();
    let cap = outer_re.captures(perm)?;
    let pattern = &cap[1];
    let escaped = regex::escape(pattern).replace(r"\*", ".*");
    let full = format!("^{}$", escaped);
    Regex::new(&full).ok()
}

// --- Terminal TTY detection ---

/// Walk up the process tree to find the terminal tty.
///
/// When invoked via Claude Code -> bash -> bin/flow -> rust, the immediate
/// parent has no controlling terminal (tty shows '??'). Walking up the
/// process tree finds the first ancestor with a real tty.
pub fn detect_tty() -> Option<String> {
    detect_tty_with(&mut |pid| run_ps_for_pid("ps", pid))
}

/// Testable seam for the `ps`-spawning half of `detect_tty`. Accepts
/// the command name so tests can drive the spawn-failure branch by
/// passing a nonexistent binary.
pub fn run_ps_for_pid(cmd: &str, pid: u32) -> Option<String> {
    let output = std::process::Command::new(cmd)
        .args(["-o", "tty=,ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    check_ps_output(&output)
}

/// Interpret a completed `ps` subprocess result. Returns the stdout
/// string on success, `None` when the process exited non-zero.
/// Extracted as a pub seam so tests can cover the non-success branch
/// by passing an `Output` from a command known to exit non-zero.
pub fn check_ps_output(output: &std::process::Output) -> Option<String> {
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Testable seam for `detect_tty`. Accepts a closure that returns the
/// `ps -o tty=,ppid=` stdout for a given pid, or `None` when the
/// subprocess cannot be used. Tests drive each loop branch by supplying
/// a mock closure.
pub fn detect_tty_with(ps: &mut dyn FnMut(u32) -> Option<String>) -> Option<String> {
    let mut pid = std::process::id();
    for _ in 0..20 {
        let stdout = match ps(pid) {
            Some(s) => s,
            None => break,
        };
        let parts: Vec<&str> = stdout.split_whitespace().collect();
        if parts.len() < 2 {
            break;
        }
        let tty = parts[0];
        let ppid = parts[1];
        if tty != "??" && tty != "?" {
            return Some(format!("/dev/{}", tty));
        }
        pid = ppid.parse().ok()?;
        if pid <= 1 {
            break;
        }
    }
    None
}

// --- Tab color functions ---

/// Return an (r, g, b) tuple for the terminal tab color.
///
/// Precedence: override > pinned > hash.
pub fn format_tab_color(
    repo: Option<&str>,
    override_color: Option<(u8, u8, u8)>,
) -> Option<(u8, u8, u8)> {
    if let Some(color) = override_color {
        return Some(color);
    }

    let repo = repo?;
    if repo.is_empty() {
        return None;
    }

    if let Some(color) = pinned_color(repo) {
        return Some(color);
    }

    let mut hasher = Sha256::new();
    hasher.update(repo.as_bytes());
    let digest = hasher.finalize();
    let index = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) as usize
        % TAB_COLORS.len();
    Some(TAB_COLORS[index])
}

/// Build and write terminal tab color escape sequences to /dev/tty.
///
/// Reads .flow.json for tab_color override, computes color,
/// builds iTerm2 OSC escape sequences, and writes them to /dev/tty.
pub fn write_tab_sequences(repo: Option<&str>, root: Option<&Path>) -> Result<(), io::Error> {
    // Read .flow.json for override
    let override_color = read_flow_json_tab_color(root);

    let color = match format_tab_color(repo, override_color) {
        Some(c) => c,
        None => return Ok(()),
    };

    let (r, g, b) = color;
    let sequences = format!(
        "\x1b]6;1;bg;red;brightness;{}\x07\x1b]6;1;bg;green;brightness;{}\x07\x1b]6;1;bg;blue;brightness;{}\x07",
        r, g, b
    );

    fs::write("/dev/tty", sequences)
}

/// Read tab_color override from .flow.json.
fn read_flow_json_tab_color(root: Option<&Path>) -> Option<(u8, u8, u8)> {
    let path = match root {
        Some(r) => r.join(".flow.json"),
        None => std::path::PathBuf::from(".flow.json"),
    };
    let content = fs::read_to_string(path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&content).ok()?;
    let arr = data.get("tab_color")?.as_array()?;
    if arr.len() == 3 {
        let r = arr[0].as_u64()? as u8;
        let g = arr[1].as_u64()? as u8;
        let b = arr[2].as_u64()? as u8;
        Some((r, g, b))
    } else {
        None
    }
}

/// Detect dev mode from .flow.json (presence of plugin_root_backup key).
pub fn detect_dev_mode(root: &Path) -> bool {
    let flow_json_path = root.join(".flow.json");
    if !flow_json_path.exists() {
        return false;
    }
    match std::fs::read_to_string(&flow_json_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(data) => data.get("plugin_root_backup").is_some(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

// --- tolerant_i64 ---

/// Read a JSON value as i64, tolerating int, float, and string representations.
///
/// State files can outlive the code that writes them. Accepts all three
/// representations so counter fields survive round-trips through external
/// editors or legacy writers that store numbers as strings or floats.
/// Returns `None` when the value cannot be interpreted as a number (bool,
/// null, object, array, or unparseable string). Callers that need "data
/// not available" vs "present with value 0" should use this function.
pub fn tolerant_i64_opt(v: &serde_json::Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Read a JSON value as i64 with 0 as the default.
///
/// Thin `unwrap_or(0)` wrapper over [`tolerant_i64_opt`] for counter fields
/// where a missing or unparseable value should mean zero rather than "data
/// not available".
pub fn tolerant_i64(v: &serde_json::Value) -> i64 {
    tolerant_i64_opt(v).unwrap_or(0)
}
