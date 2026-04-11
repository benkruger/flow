use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::America::Los_Angeles;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

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

/// Polling-based wait_timeout for child processes.
trait WaitTimeout {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::thread;

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match self.try_wait()? {
                Some(status) => {
                    return Ok(Some(status));
                }
                None => {
                    if start.elapsed() >= dur {
                        return Ok(None);
                    }
                    thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
            }
        }
    }
}

/// Run a shell command with optional timeout, returning (stdout, stderr).
/// Used by start-workspace (worktree/PR creation) and auto-close-parent.
pub fn run_cmd(
    args: &[&str],
    cwd: &Path,
    step_name: &str,
    timeout: Option<Duration>,
) -> Result<(String, String), SetupError> {
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

    if let Some(dur) = timeout {
        match child.wait_timeout(dur) {
            Ok(Some(status)) => {
                let output = child.wait_with_output().map_err(|e| SetupError {
                    step: step_name.to_string(),
                    message: e.to_string(),
                })?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return Err(SetupError {
                        step: step_name.to_string(),
                        message: if stderr.is_empty() { stdout } else { stderr },
                    });
                }
                Ok((
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(SetupError {
                    step: step_name.to_string(),
                    message: format!("Timed out after {}s", dur.as_secs()),
                })
            }
            Err(e) => Err(SetupError {
                step: step_name.to_string(),
                message: e.to_string(),
            }),
        }
    } else {
        let output = child.wait_with_output().map_err(|e| SetupError {
            step: step_name.to_string(),
            message: e.to_string(),
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(SetupError {
                step: step_name.to_string(),
                message: if stderr.is_empty() { stdout } else { stderr },
            });
        }
        Ok((
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
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

/// Read plugin version from plugin.json next to the Rust binary.
///
/// Navigates up from the binary location (target/{release|debug}/flow-rs)
/// to find .claude-plugin/plugin.json.
pub fn read_version() -> String {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return "?".to_string(),
    };
    // Binary is at <plugin_root>/target/{release|debug}/flow-rs
    // Go up 3 levels: flow-rs -> {release|debug} -> target -> plugin_root
    let plugin_root = match exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        Some(r) => r,
        None => return "?".to_string(),
    };
    let plugin_json = plugin_root.join(".claude-plugin").join("plugin.json");
    read_version_from(&plugin_json)
}

// --- Plugin root ---

/// Find the plugin root directory (where flow-phases.json lives).
///
/// Checks CLAUDE_PLUGIN_ROOT env var first, then walks up from the
/// current executable's location.
pub fn plugin_root() -> Option<std::path::PathBuf> {
    if let Ok(root) = std::env::var("CLAUDE_PLUGIN_ROOT") {
        let path = std::path::PathBuf::from(&root);
        if path.join("flow-phases.json").exists() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
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
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent()?.parent()?.parent().map(|d| d.to_path_buf()))
        .map(|d: std::path::PathBuf| d.join("bin").join("flow"))
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "bin/flow".to_string())
}

/// Return the frameworks directory inside the plugin root.
///
/// Returns `<plugin_root>/frameworks` or None if plugin root cannot be found.
pub fn frameworks_dir() -> Option<std::path::PathBuf> {
    plugin_root().map(|r| r.join("frameworks"))
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
        Err(_) => {
            // Try parsing ISO 8601 with chrono's flexible parser
            match started.parse::<DateTime<FixedOffset>>() {
                Ok(dt) => dt,
                Err(_) => return 0,
            }
        }
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
/// Uses a 10-second timeout matching the Python implementation.
pub fn fetch_issue_info(issue_number: i64) -> Option<IssueInfo> {
    let dir = std::env::current_dir().ok()?;
    let (stdout, _) = run_cmd(
        &[
            "gh",
            "issue",
            "view",
            &issue_number.to_string(),
            "--json",
            "title,labels",
            "--jq",
            "{title, labels: [.labels[].name]}",
        ],
        &dir,
        "fetch_issue_info",
        Some(Duration::from_secs(10)),
    )
    .ok()?;

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
    let state_dir = project_root.join(".flow-states");
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
    let mut pid = std::process::id();
    for _ in 0..20 {
        let output = std::process::Command::new("ps")
            .args(["-o", "tty=,ppid=", "-p", &pid.to_string()])
            .output()
            .ok()?;

        if !output.status.success() {
            break;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // --- now() ---

    #[test]
    fn now_returns_iso8601_pacific() {
        let ts = now();
        // Should parse as a valid datetime
        assert!(DateTime::parse_from_rfc3339(&ts).is_ok() || ts.contains('T'));
        // Should contain timezone offset (not Z for UTC)
        assert!(ts.contains('-') || ts.contains('+'));
        // Should not end with Z
        assert!(!ts.ends_with('Z'));
    }

    // --- format_time() ---

    #[test]
    fn format_time_under_60_seconds() {
        assert_eq!(format_time(0), "<1m");
        assert_eq!(format_time(30), "<1m");
        assert_eq!(format_time(59), "<1m");
    }

    #[test]
    fn format_time_exactly_60_seconds() {
        assert_eq!(format_time(60), "1m");
    }

    #[test]
    fn format_time_minutes_only() {
        assert_eq!(format_time(120), "2m");
        assert_eq!(format_time(3599), "59m");
    }

    #[test]
    fn format_time_hours_and_minutes() {
        assert_eq!(format_time(3600), "1h 0m");
        assert_eq!(format_time(3660), "1h 1m");
        assert_eq!(format_time(7200), "2h 0m");
        assert_eq!(format_time(7380), "2h 3m");
    }

    #[test]
    fn format_time_large_values() {
        assert_eq!(format_time(36000), "10h 0m");
    }

    #[test]
    fn format_time_negative() {
        assert_eq!(format_time(-1), "?");
    }

    // --- elapsed_since() ---

    #[test]
    fn elapsed_since_none() {
        assert_eq!(elapsed_since(None, None), 0);
    }

    #[test]
    fn elapsed_since_empty_string() {
        assert_eq!(elapsed_since(Some(""), None), 0);
    }

    #[test]
    fn elapsed_since_with_explicit_now() {
        let started = "2026-01-01T00:00:00-08:00";
        let now_dt = FixedOffset::west_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 0, 10, 0)
            .unwrap();
        assert_eq!(elapsed_since(Some(started), Some(now_dt)), 600);
    }

    #[test]
    fn elapsed_since_default_now() {
        let result = elapsed_since(Some("2026-01-01T00:00:00-08:00"), None);
        assert!(result >= 0);
    }

    #[test]
    fn elapsed_since_utc_timestamp() {
        let started = "2026-01-01T00:00:00+00:00";
        let now_dt = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 0, 5, 0)
            .unwrap();
        assert_eq!(elapsed_since(Some(started), Some(now_dt)), 300);
    }

    #[test]
    fn elapsed_since_never_negative() {
        let started = "2026-01-01T01:00:00-08:00";
        let now_dt = FixedOffset::west_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .unwrap();
        assert_eq!(elapsed_since(Some(started), Some(now_dt)), 0);
    }

    // --- branch_name() ---

    #[test]
    fn branch_name_basic() {
        assert_eq!(branch_name("invoice pdf export"), "invoice-pdf-export");
    }

    #[test]
    fn branch_name_special_chars() {
        assert_eq!(branch_name("fix login timeout!"), "fix-login-timeout");
    }

    #[test]
    fn branch_name_max_32_chars() {
        let long = "fix login timeout when session expires after thirty minutes";
        let result = branch_name(long);
        assert!(result.len() <= 32, "Got: {} ({})", result, result.len());
        // Should truncate on hyphen boundary
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn branch_name_preserves_hyphens() {
        assert_eq!(branch_name("my-feature"), "my-feature");
    }

    #[test]
    fn branch_name_strips_non_alphanumeric() {
        assert_eq!(branch_name("hello @world #123"), "hello-world-123");
    }

    #[test]
    fn branch_name_multibyte_no_panic() {
        // Multi-byte chars are stripped by the regex, so the result is ASCII.
        // This test verifies no panic from byte-offset slicing on multi-byte input.
        let input = "fix 日本語 login timeout when session expires after thirty minutes";
        let result = branch_name(input);
        assert!(result.len() <= 32, "Got: {} ({})", result, result.len());
        assert!(result.is_ascii());
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn branch_name_empty_string() {
        let result = branch_name("");
        assert_eq!(result, "unnamed");
    }

    #[test]
    fn branch_name_all_special_chars() {
        let result = branch_name("!@#$%");
        assert_eq!(result, "unnamed");
    }

    #[test]
    fn branch_name_reserved_words_pass_through() {
        // Reserved words like HEAD and main are valid branch components
        // when used as feature descriptions
        assert_eq!(branch_name("HEAD"), "head");
        assert_eq!(branch_name("main"), "main");
    }

    // --- derive_feature() ---

    #[test]
    fn derive_feature_basic() {
        assert_eq!(derive_feature("invoice-pdf-export"), "Invoice Pdf Export");
    }

    #[test]
    fn derive_feature_single_word() {
        assert_eq!(derive_feature("fix"), "Fix");
    }

    // --- derive_worktree() ---

    #[test]
    fn derive_worktree_basic() {
        assert_eq!(derive_worktree("my-feature"), ".worktrees/my-feature");
    }

    // --- extract_issue_numbers() ---

    #[test]
    fn extract_issue_numbers_hash_pattern() {
        assert_eq!(extract_issue_numbers("fix #42 and #99"), vec![42, 99]);
    }

    #[test]
    fn extract_issue_numbers_url_pattern() {
        assert_eq!(
            extract_issue_numbers("see https://github.com/org/repo/issues/123"),
            vec![123]
        );
    }

    #[test]
    fn extract_issue_numbers_mixed() {
        assert_eq!(
            extract_issue_numbers("fix #42 see /issues/99"),
            vec![42, 99]
        );
    }

    #[test]
    fn extract_issue_numbers_dedup() {
        assert_eq!(extract_issue_numbers("#42 and #42"), vec![42]);
    }

    #[test]
    fn extract_issue_numbers_none() {
        assert_eq!(extract_issue_numbers("no issues here"), Vec::<i64>::new());
    }

    // --- short_issue_ref() ---

    #[test]
    fn short_issue_ref_github_url() {
        assert_eq!(
            short_issue_ref("https://github.com/org/repo/issues/42"),
            "#42"
        );
    }

    #[test]
    fn short_issue_ref_non_github() {
        assert_eq!(
            short_issue_ref("https://example.com/other"),
            "https://example.com/other"
        );
    }

    // --- read_prompt_file() ---

    #[test]
    fn read_prompt_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.txt");
        fs::write(&path, "hello world").unwrap();
        let result = read_prompt_file(&path).unwrap();
        assert_eq!(result, "hello world");
        assert!(!path.exists(), "File should be deleted after read");
    }

    #[test]
    fn read_prompt_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.txt");
        assert!(read_prompt_file(&path).is_err());
    }

    // --- parse_conflict_files() ---

    #[test]
    fn parse_conflict_uu() {
        assert_eq!(
            parse_conflict_files("UU src/main.rs\n"),
            vec!["src/main.rs"]
        );
    }

    #[test]
    fn parse_conflict_aa_dd() {
        let output = "AA src/new.rs\nDD src/old.rs\n";
        let result = parse_conflict_files(output);
        assert_eq!(result, vec!["src/new.rs", "src/old.rs"]);
    }

    #[test]
    fn parse_conflict_u_in_status() {
        // DU means deleted on one side, unmerged on other
        assert_eq!(
            parse_conflict_files("DU src/file.rs\n"),
            vec!["src/file.rs"]
        );
    }

    #[test]
    fn parse_conflict_no_conflicts() {
        assert_eq!(
            parse_conflict_files("M  src/lib.rs\nA  src/new.rs\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parse_conflict_empty() {
        assert_eq!(parse_conflict_files(""), Vec::<String>::new());
    }

    // --- permission_to_regex() ---

    #[test]
    fn permission_to_regex_basic() {
        let re = permission_to_regex("Bash(git push)").unwrap();
        assert!(re.is_match("git push"));
        assert!(!re.is_match("git pull"));
    }

    #[test]
    fn permission_to_regex_wildcard() {
        let re = permission_to_regex("Bash(git push *)").unwrap();
        assert!(re.is_match("git push origin main"));
        assert!(!re.is_match("git pull"));
    }

    #[test]
    fn permission_to_regex_semicolon_wildcard() {
        let re = permission_to_regex("Bash(bin/ci;*)").unwrap();
        assert!(re.is_match("bin/ci; echo done"));
        assert!(!re.is_match("bin/test"));
    }

    #[test]
    fn permission_to_regex_non_bash() {
        let re = permission_to_regex("Read(file.txt)").unwrap();
        assert!(re.is_match("file.txt"));
        assert!(!re.is_match("other.txt"));
    }

    #[test]
    fn permission_to_regex_read_wildcard() {
        let re = permission_to_regex("Read(~/.claude/rules/*)").unwrap();
        assert!(re.is_match("~/.claude/rules/foo.md"));
        assert!(!re.is_match("~/.claude/other/bar.md"));
    }

    #[test]
    fn permission_to_regex_agent() {
        let re = permission_to_regex("Agent(*)").unwrap();
        assert!(re.is_match("flow:ci-fixer"));
        assert!(re.is_match("anything"));
    }

    #[test]
    fn permission_to_regex_skill() {
        let re = permission_to_regex("Skill(decompose:decompose)").unwrap();
        assert!(re.is_match("decompose:decompose"));
        assert!(!re.is_match("decompose:other"));
    }

    #[test]
    fn permission_to_regex_double_star() {
        let re = permission_to_regex("Read(~/.claude/projects/**/tool-results/*)").unwrap();
        assert!(re.is_match("~/.claude/projects/foo/bar/tool-results/abc"));
        assert!(!re.is_match("~/.claude/other/tool-results/abc"));
    }

    #[test]
    fn permission_to_regex_exact_match_only() {
        let re = permission_to_regex("Bash(git push)").unwrap();
        assert!(!re.is_match("git push origin"));
    }

    // --- format_tab_color() ---

    #[test]
    fn format_tab_color_override_wins() {
        let color = format_tab_color(Some("any/repo"), Some((255, 0, 0)));
        assert_eq!(color, Some((255, 0, 0)));
    }

    #[test]
    fn format_tab_color_pinned() {
        let color = format_tab_color(Some("benkruger/flow"), None);
        assert_eq!(color, Some((40, 180, 70)));
    }

    #[test]
    fn format_tab_color_hash_based() {
        let color = format_tab_color(Some("org/some-random-repo"), None);
        assert!(color.is_some());
        // Should be one of the TAB_COLORS
        assert!(TAB_COLORS.contains(&color.unwrap()));
    }

    #[test]
    fn format_tab_color_deterministic() {
        let c1 = format_tab_color(Some("org/repo"), None);
        let c2 = format_tab_color(Some("org/repo"), None);
        assert_eq!(c1, c2);
    }

    #[test]
    fn format_tab_color_none_for_empty_repo() {
        assert_eq!(format_tab_color(Some(""), None), None);
        assert_eq!(format_tab_color(None, None), None);
    }

    // --- pinned_color() ---

    #[test]
    fn pinned_color_known_repos() {
        assert_eq!(pinned_color("HipaaHealth/mono-repo"), Some((50, 120, 220)));
        assert_eq!(
            pinned_color("benkruger/salted-kitchen"),
            Some((220, 130, 20))
        );
        assert_eq!(pinned_color("benkruger/flow"), Some((40, 180, 70)));
    }

    #[test]
    fn pinned_color_unknown_repo() {
        assert_eq!(pinned_color("unknown/repo"), None);
    }

    // --- check_duplicate_issue() ---

    #[test]
    fn check_duplicate_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_duplicate_issue(dir.path(), &[] as &[i64], "any").is_none());
    }

    #[test]
    fn check_duplicate_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "any").is_none());
    }

    #[test]
    fn check_duplicate_detects_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            serde_json::json!({
                "prompt": "work on issue #123",
                "branch": "existing-branch",
                "current_phase": "flow-code",
                "pr_url": "https://github.com/test/repo/pull/99",
            })
            .to_string(),
        )
        .unwrap();
        let result = check_duplicate_issue(dir.path(), &[123], "new-branch");
        assert!(result.is_some());
        let dup = result.unwrap();
        assert_eq!(dup.branch, "existing-branch");
        assert_eq!(dup.phase, "flow-code");
        assert_eq!(dup.pr_url, "https://github.com/test/repo/pull/99");
    }

    #[test]
    fn check_duplicate_no_false_positive() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            serde_json::json!({
                "prompt": "work on issue #123",
                "branch": "existing-branch",
                "current_phase": "flow-code",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[456], "new-branch").is_none());
    }

    #[test]
    fn check_duplicate_multi_issue_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            serde_json::json!({
                "prompt": "work on issue #456",
                "branch": "existing-branch",
                "current_phase": "flow-plan",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        let result = check_duplicate_issue(dir.path(), &[123, 456], "new-branch");
        assert!(result.is_some());
    }

    #[test]
    fn check_duplicate_skips_self_branch() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("my-branch.json"),
            serde_json::json!({
                "prompt": "work on issue #123",
                "branch": "my-branch",
                "current_phase": "flow-start",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "my-branch").is_none());
    }

    #[test]
    fn check_duplicate_skips_phases_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("some-branch-phases.json"),
            serde_json::json!({
                "prompt": "work on issue #123",
                "branch": "some-branch",
                "current_phase": "flow-code",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn check_duplicate_skips_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("bad-json.json"), "not valid json {{{").unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn check_duplicate_null_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("null-prompt.json"),
            serde_json::json!({"prompt": null, "branch": "null-prompt"}).to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn check_duplicate_skips_completed_flow() {
        // A completed flow's leftover state file should not block a new flow
        // targeting the same issue
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("completed-branch.json"),
            serde_json::json!({
                "prompt": "work on issue #42",
                "branch": "completed-branch",
                "current_phase": "flow-complete",
                "phases": {
                    "flow-complete": {
                        "status": "complete"
                    }
                },
                "pr_url": "https://github.com/test/repo/pull/55",
            })
            .to_string(),
        )
        .unwrap();
        assert!(
            check_duplicate_issue(dir.path(), &[42], "new-branch").is_none(),
            "Completed flow should not block new flow for the same issue"
        );
    }

    #[test]
    fn check_duplicate_skips_empty_state_file() {
        // An empty (0-byte) state file should not block
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("empty-branch.json"), "").unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    // --- IssueInfo deserialization (issue #887) ---

    #[test]
    fn fetch_issue_info_struct_deserializes_full() {
        let json = r#"{"title": "Some Issue", "labels": ["bug", "Flow In-Progress"]}"#;
        let info: IssueInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.title, "Some Issue");
        assert_eq!(
            info.labels,
            vec!["bug".to_string(), "Flow In-Progress".to_string()]
        );
    }

    #[test]
    fn fetch_issue_info_struct_deserializes_missing_labels() {
        // When the `labels` key is absent, `#[serde(default)]` must yield an empty vec
        // rather than failing deserialization. This is the defensive default for
        // older gh versions or shapes where jq does not emit the key.
        let json = r#"{"title": "Some Issue"}"#;
        let info: IssueInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.title, "Some Issue");
        assert!(info.labels.is_empty());
    }

    #[test]
    fn fetch_issue_info_struct_deserializes_null_labels() {
        // When `labels` is explicitly null, `#[serde(default)]` combined with
        // the `Vec<String>` field type must still coerce to empty. Without this,
        // a label-guard failure would surface as a misleading fetch error.
        let json = r#"{"title": "Some Issue", "labels": null}"#;
        let info: IssueInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.title, "Some Issue");
        assert!(info.labels.is_empty());
    }

    // --- read_version_from() ---

    #[test]
    fn read_version_from_valid_plugin_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, r#"{"version": "1.2.3"}"#).unwrap();
        assert_eq!(read_version_from(&path), "1.2.3");
    }

    #[test]
    fn read_version_from_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        assert_eq!(read_version_from(&path), "?");
    }

    #[test]
    fn read_version_from_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, "{bad json").unwrap();
        assert_eq!(read_version_from(&path), "?");
    }

    #[test]
    fn read_version_from_no_version_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        fs::write(&path, r#"{"name": "flow"}"#).unwrap();
        assert_eq!(read_version_from(&path), "?");
    }

    // --- tolerant_i64_opt() / tolerant_i64() ---

    #[test]
    fn tolerant_i64_opt_accepts_int() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!(42)), Some(42));
        assert_eq!(tolerant_i64_opt(&serde_json::json!(-7)), Some(-7));
        assert_eq!(tolerant_i64_opt(&serde_json::json!(0)), Some(0));
    }

    #[test]
    fn tolerant_i64_opt_accepts_float_truncates() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!(3.7)), Some(3));
        assert_eq!(tolerant_i64_opt(&serde_json::json!(1.0)), Some(1));
        assert_eq!(tolerant_i64_opt(&serde_json::json!(-2.9)), Some(-2));
    }

    #[test]
    fn tolerant_i64_opt_accepts_string_numeric() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!("123")), Some(123));
        assert_eq!(tolerant_i64_opt(&serde_json::json!("0")), Some(0));
    }

    #[test]
    fn tolerant_i64_opt_accepts_negative_string() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!("-5")), Some(-5));
    }

    #[test]
    fn tolerant_i64_opt_returns_none_for_bool() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!(true)), None);
        assert_eq!(tolerant_i64_opt(&serde_json::json!(false)), None);
    }

    #[test]
    fn tolerant_i64_opt_returns_none_for_null() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!(null)), None);
    }

    #[test]
    fn tolerant_i64_opt_returns_none_for_unparseable_string() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!("garbage")), None);
        assert_eq!(tolerant_i64_opt(&serde_json::json!("")), None);
    }

    #[test]
    fn tolerant_i64_opt_returns_none_for_array() {
        assert_eq!(tolerant_i64_opt(&serde_json::json!([1, 2, 3])), None);
        assert_eq!(tolerant_i64_opt(&serde_json::json!({})), None);
    }

    #[test]
    fn tolerant_i64_passes_through_when_opt_returns_some() {
        assert_eq!(tolerant_i64(&serde_json::json!(42)), 42);
        assert_eq!(tolerant_i64(&serde_json::json!("5")), 5);
        assert_eq!(tolerant_i64(&serde_json::json!(7.9)), 7);
    }

    #[test]
    fn tolerant_i64_defaults_zero_when_opt_returns_none() {
        assert_eq!(tolerant_i64(&serde_json::json!(null)), 0);
        assert_eq!(tolerant_i64(&serde_json::json!(true)), 0);
        assert_eq!(tolerant_i64(&serde_json::json!("garbage")), 0);
        assert_eq!(tolerant_i64(&serde_json::json!([])), 0);
    }

    #[test]
    fn tolerant_i64_zero_for_missing_via_index() {
        // `IndexMut` on a missing key returns Value::Null; tolerant_i64 should
        // coerce to 0 so callers can use `state["missing"]` directly.
        let state = serde_json::json!({"branch": "test"});
        assert_eq!(tolerant_i64(&state["missing_key"]), 0);
    }
}
