//! Pure data layer for the FLOW interactive TUI.
//!
//! Reads state files, computes display structs (flow summaries, phase timelines,
//! log entries). No curses dependency — fully testable with make_state() fixture.
//!
//! Ported from lib/tui_data.py. Consumed by lib/tui.py via the `bin/flow tui-data`
//! CLI subcommand bridge.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use serde_json::Value;

use crate::phase_config::{self, PHASE_ORDER};
use crate::utils::{
    derive_feature, derive_worktree, elapsed_since, extract_issue_numbers, format_time,
    short_issue_ref,
};

/// Static mapping of (phase_key, display_step_number) → short step name.
///
/// Display step number is what the user sees in the annotation.
/// Source: skill SKILL.md step headings (## Step N — Name).
pub fn step_names() -> HashMap<&'static str, HashMap<i64, &'static str>> {
    let mut map = HashMap::new();

    let mut start = HashMap::new();
    start.insert(3, "creating state");
    start.insert(4, "labeling issues");
    start.insert(5, "pulling main");
    start.insert(6, "running CI");
    start.insert(7, "updating deps");
    start.insert(8, "CI after deps");
    start.insert(9, "committing");
    start.insert(10, "releasing lock");
    start.insert(11, "setting up workspace");
    map.insert("flow-start", start);

    let mut plan = HashMap::new();
    plan.insert(1, "reading context");
    plan.insert(2, "decomposing");
    plan.insert(3, "writing plan");
    plan.insert(4, "storing plan");
    map.insert("flow-plan", plan);

    let mut code_review = HashMap::new();
    code_review.insert(1, "simplifying");
    code_review.insert(2, "reviewing");
    code_review.insert(3, "security review");
    code_review.insert(4, "agent reviews");
    map.insert("flow-code-review", code_review);

    let mut learn = HashMap::new();
    learn.insert(1, "gathering sources");
    learn.insert(2, "synthesizing");
    learn.insert(3, "applying learnings");
    learn.insert(4, "promoting perms");
    learn.insert(5, "committing");
    learn.insert(6, "filing issues");
    learn.insert(7, "presenting report");
    map.insert("flow-learn", learn);

    let mut complete = HashMap::new();
    complete.insert(1, "checking state");
    complete.insert(2, "checking PR");
    complete.insert(3, "merging main");
    complete.insert(4, "running CI");
    complete.insert(5, "checking GitHub CI");
    complete.insert(6, "confirming merge");
    complete.insert(7, "archiving to PR");
    complete.insert(8, "merging PR");
    complete.insert(9, "closing issues");
    complete.insert(10, "post-merge ops");
    complete.insert(11, "cleaning up");
    complete.insert(12, "pulling changes");
    map.insert("flow-complete", complete);

    map
}

/// Status icons for orchestration queue items.
pub fn status_icon(status: &str) -> &'static str {
    match status {
        "completed" => "\u{2713}",
        "failed" => "\u{2717}",
        "in_progress" => "\u{25b6}",
        _ => "\u{00b7}",
    }
}

/// Staleness threshold for rate limit data (10 minutes).
pub const STALE_THRESHOLD_SECONDS: u64 = 600;

/// Return 'name - step N of M' or 'step N of M' or '' depending on what's populated.
pub fn step_annotation(step: i64, total: i64, name: &str) -> String {
    if step <= 0 {
        return String::new();
    }
    let step_str = if total > 0 {
        format!("step {} of {}", step, total)
    } else {
        format!("step {}", step)
    };
    if !name.is_empty() {
        format!("{} - {}", name, step_str)
    } else {
        step_str
    }
}

/// A single entry in the phase timeline display.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub key: String,
    pub name: String,
    pub number: usize,
    pub status: String,
    pub time: String,
    pub annotation: String,
}

/// Build a list of phase display entries from a state dict.
pub fn phase_timeline(state: &Value, now: Option<DateTime<FixedOffset>>) -> Vec<TimelineEntry> {
    let now = now.unwrap_or_else(|| {
        use chrono::Utc;
        use chrono_tz::America::Los_Angeles;
        Utc::now().with_timezone(&Los_Angeles).fixed_offset()
    });

    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => return vec![],
    };

    let names_map = phase_config::phase_names();
    let numbers_map = phase_config::phase_numbers();
    let all_step_names = step_names();

    let start_step = state
        .get("start_step")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let start_steps_total = state
        .get("start_steps_total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let plan_step = state.get("plan_step").and_then(|v| v.as_i64()).unwrap_or(0);
    let plan_steps_total = state
        .get("plan_steps_total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let code_task = state.get("code_task").and_then(|v| v.as_i64()).unwrap_or(0);
    let code_tasks_total = state
        .get("code_tasks_total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let code_task_name = state
        .get("code_task_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let code_review_step = state
        .get("code_review_step")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let learn_step = state
        .get("learn_step")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let learn_steps_total = state
        .get("learn_steps_total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let complete_step = state
        .get("complete_step")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let complete_steps_total = state
        .get("complete_steps_total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let diff_stats = state.get("diff_stats");

    let mut entries = Vec::new();

    for &key in PHASE_ORDER {
        let phase = match phases.get(key) {
            Some(p) => p,
            None => continue,
        };
        let status = phase
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let mut seconds = phase
            .get("cumulative_seconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let number = numbers_map.get(key).copied().unwrap_or(0);
        let name = names_map
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string());

        let time_str = if status == "complete" {
            format_time(seconds)
        } else if status == "in_progress" {
            let session_started = phase
                .get("session_started_at")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty());
            if let Some(ss) = session_started {
                seconds += elapsed_since(Some(ss), Some(now));
            }
            if seconds > 0 {
                format_time(seconds)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let annotation = if status != "in_progress" {
            String::new()
        } else if key == "flow-start" {
            let sn = all_step_names
                .get("flow-start")
                .and_then(|m| m.get(&start_step))
                .copied()
                .unwrap_or("");
            step_annotation(start_step, start_steps_total, sn)
        } else if key == "flow-plan" {
            let sn = all_step_names
                .get("flow-plan")
                .and_then(|m| m.get(&plan_step))
                .copied()
                .unwrap_or("");
            step_annotation(plan_step, plan_steps_total, sn)
        } else if key == "flow-code" {
            let mut current_task = code_task + 1;
            if code_tasks_total > 0 {
                current_task = current_task.min(code_tasks_total);
            }
            let task_str = if code_tasks_total > 0 {
                format!("task {} of {}", current_task, code_tasks_total)
            } else {
                format!("task {}", current_task)
            };
            let task_str = if !code_task_name.is_empty() {
                // Truncate by char count, not byte count (Python parity)
                let truncated: String = if code_task_name.chars().count() > 30 {
                    let prefix: String = code_task_name.chars().take(27).collect();
                    format!("{}...", prefix)
                } else {
                    code_task_name.to_string()
                };
                format!("{} - {}", truncated, task_str)
            } else {
                task_str
            };
            let mut parts = vec![task_str];
            if let Some(ds) = diff_stats {
                let ins = ds.get("insertions").and_then(|v| v.as_i64()).unwrap_or(0);
                let dels = ds.get("deletions").and_then(|v| v.as_i64()).unwrap_or(0);
                parts.push(format!("+{} -{}", ins, dels));
            }
            parts.join(", ")
        } else if key == "flow-code-review" {
            let cr_total = all_step_names
                .get("flow-code-review")
                .map(|m| m.len() as i64)
                .unwrap_or(0);
            let display_step = code_review_step + 1;
            if display_step <= cr_total {
                let sn = all_step_names
                    .get("flow-code-review")
                    .and_then(|m| m.get(&display_step))
                    .copied()
                    .unwrap_or("");
                step_annotation(display_step, cr_total, sn)
            } else {
                String::new()
            }
        } else if key == "flow-learn" {
            let display_step = learn_step + 1;
            let sn = all_step_names
                .get("flow-learn")
                .and_then(|m| m.get(&display_step))
                .copied()
                .unwrap_or("");
            step_annotation(display_step, learn_steps_total, sn)
        } else if key == "flow-complete" {
            let sn = all_step_names
                .get("flow-complete")
                .and_then(|m| m.get(&complete_step))
                .copied()
                .unwrap_or("");
            step_annotation(complete_step, complete_steps_total, sn)
        } else {
            String::new()
        };

        entries.push(TimelineEntry {
            key: key.to_string(),
            name,
            number,
            status: status.to_string(),
            time: time_str,
            annotation,
        });
    }

    entries
}

/// A parsed log entry for display.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub time: String,
    pub message: String,
}

/// Parse log file content into display entries.
///
/// Each log line has format: `<ISO8601-Pacific> <message>`
/// Returns last `limit` entries as LogEntry structs.
pub fn parse_log_entries(log_content: &str, limit: usize) -> Vec<LogEntry> {
    if log_content.is_empty() {
        return vec![];
    }

    let re = regex::Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[^\s]*)\s+(.+)$").unwrap();
    let mut entries = Vec::new();

    for line in log_content.trim().split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let timestamp_str = caps.get(1).unwrap().as_str();
            let message = caps.get(2).unwrap().as_str();
            if let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp_str) {
                let time_display = parsed.format("%H:%M").to_string();
                entries.push(LogEntry {
                    time: time_display,
                    message: message.to_string(),
                });
            }
        }
    }

    let start = if entries.len() > limit {
        entries.len() - limit
    } else {
        0
    };
    entries[start..].to_vec()
}

/// Display-ready issue entry.
#[derive(Debug, Clone, Serialize)]
pub struct IssueSummary {
    pub label: String,
    pub title: String,
    pub url: String,
    /// Serializes as "ref" for Python parity. `ref` is a Rust keyword.
    #[serde(rename = "ref")]
    pub ref_str: String,
    pub phase_name: String,
}

/// Display-ready flow summary.
#[derive(Debug, Clone, Serialize)]
pub struct FlowSummary {
    pub feature: String,
    pub branch: String,
    pub worktree: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub phase_number: usize,
    pub phase_name: String,
    pub elapsed: String,
    pub code_task: i64,
    pub diff_stats: Option<Value>,
    pub notes_count: usize,
    pub issues_count: usize,
    pub issues: Vec<IssueSummary>,
    pub blocked: bool,
    pub issue_numbers: Vec<i64>,
    pub plan_path: Option<String>,
    pub annotation: String,
    pub phase_elapsed: String,
    pub timeline: Vec<TimelineEntry>,
    /// Raw state dict — needed by tui.py for detail views.
    pub state: Value,
}

/// Convert a state dict to a display-ready summary.
pub fn flow_summary(state: &Value, now: Option<DateTime<FixedOffset>>) -> FlowSummary {
    let now = now.unwrap_or_else(|| {
        use chrono::Utc;
        use chrono_tz::America::Los_Angeles;
        Utc::now().with_timezone(&Los_Angeles).fixed_offset()
    });

    let branch = state.get("branch").and_then(|b| b.as_str()).unwrap_or("");
    let current_phase = state
        .get("current_phase")
        .and_then(|p| p.as_str())
        .unwrap_or("flow-start");

    let elapsed_seconds =
        elapsed_since(state.get("started_at").and_then(|s| s.as_str()), Some(now));

    let issues_filed = state
        .get("issues_filed")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let issues: Vec<IssueSummary> = issues_filed
        .iter()
        .map(|entry| {
            let url = entry.get("url").and_then(|u| u.as_str()).unwrap_or("");
            IssueSummary {
                label: entry
                    .get("label")
                    .and_then(|l| l.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: entry
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: url.to_string(),
                ref_str: short_issue_ref(url),
                phase_name: entry
                    .get("phase_name")
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect();

    let files = state.get("files").and_then(|f| f.as_object());
    let plan_path = files
        .and_then(|f| f.get("plan"))
        .and_then(|p| p.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            state
                .get("plan_file")
                .and_then(|p| p.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(|s| s.to_string());

    let timeline = phase_timeline(state, Some(now));
    let annotation = timeline
        .iter()
        .find(|e| e.key == current_phase)
        .map(|e| e.annotation.clone())
        .unwrap_or_default();
    let phase_elapsed = timeline
        .iter()
        .find(|e| e.key == current_phase && e.status == "in_progress")
        .map(|e| e.time.clone())
        .unwrap_or_default();

    let numbers_map = phase_config::phase_numbers();
    let names_map = phase_config::phase_names();

    let notes = state
        .get("notes")
        .and_then(|n| n.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let blocked = state
        .get("_blocked")
        .map(|v| {
            // Python: bool(state.get("_blocked")) — truthy for non-empty strings, false for ""
            match v {
                Value::String(s) => !s.is_empty(),
                Value::Null => false,
                Value::Bool(b) => *b,
                _ => true,
            }
        })
        .unwrap_or(false);

    let prompt = state.get("prompt").and_then(|p| p.as_str()).unwrap_or("");

    FlowSummary {
        feature: derive_feature(branch),
        branch: branch.to_string(),
        worktree: derive_worktree(branch),
        pr_number: state.get("pr_number").and_then(|n| n.as_i64()),
        pr_url: state
            .get("pr_url")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()),
        phase_number: numbers_map
            .get(current_phase)
            .copied()
            .unwrap_or(usize::MAX),
        phase_name: names_map
            .get(current_phase)
            .cloned()
            .unwrap_or_else(|| current_phase.to_string()),
        elapsed: format_time(elapsed_seconds),
        code_task: state.get("code_task").and_then(|v| v.as_i64()).unwrap_or(0),
        diff_stats: state.get("diff_stats").cloned(),
        notes_count: notes,
        issues_count: issues_filed.len(),
        issues,
        blocked,
        issue_numbers: extract_issue_numbers(prompt),
        plan_path,
        annotation,
        phase_elapsed,
        timeline,
        state: state.clone(),
    }
}

/// Read all .flow-states/*.json state files and return flow summaries.
///
/// Returns a list of FlowSummary sorted by phase number (ascending),
/// then by feature name (alphabetical) as a tiebreaker.
/// Skips corrupt JSON and non-state files (e.g., *-phases.json).
pub fn load_all_flows(root: &Path) -> Vec<FlowSummary> {
    let state_dir = root.join(".flow-states");
    if !state_dir.is_dir() {
        return vec![];
    }

    let mut entries: Vec<_> = match std::fs::read_dir(&state_dir) {
        Ok(iter) => iter.filter_map(|e| e.ok()).collect(),
        Err(_) => return vec![],
    };
    entries.sort_by_key(|e| e.file_name());

    let mut flows = Vec::new();
    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.ends_with(".json") {
            continue;
        }
        if name_str.ends_with("-phases.json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if let Ok(state) = serde_json::from_str::<Value>(&content) {
                if state.get("branch").and_then(|b| b.as_str()).is_none() {
                    continue;
                }
                flows.push(flow_summary(&state, None));
            }
        }
    }

    flows.sort_by(|a, b| {
        a.phase_number
            .cmp(&b.phase_number)
            .then_with(|| a.feature.cmp(&b.feature))
    });
    flows
}

/// Read .flow-states/orchestrate.json and return the state dict.
///
/// Returns None if the file does not exist, is corrupt, or the state
/// directory does not exist.
pub fn load_orchestration(root: &Path) -> Option<Value> {
    let state_dir = root.join(".flow-states");
    if !state_dir.is_dir() {
        return None;
    }
    let path = state_dir.join("orchestrate.json");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Display-ready orchestration item.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationItem {
    pub icon: String,
    pub issue_number: Option<i64>,
    pub title: String,
    pub elapsed: String,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
    pub status: String,
}

/// Display-ready orchestration summary.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationSummary {
    pub elapsed: String,
    pub completed_count: usize,
    pub failed_count: usize,
    pub total: usize,
    pub is_running: bool,
    pub items: Vec<OrchestrationItem>,
}

/// Convert an orchestrate state dict to a display-ready summary.
///
/// Returns None if state is None.
pub fn orchestration_summary(
    state: Option<&Value>,
    now: Option<DateTime<FixedOffset>>,
) -> Option<OrchestrationSummary> {
    let state = state?;

    let now = now.unwrap_or_else(|| {
        use chrono::Utc;
        use chrono_tz::America::Los_Angeles;
        Utc::now().with_timezone(&Los_Angeles).fixed_offset()
    });

    let started_at = state.get("started_at").and_then(|s| s.as_str());
    let completed_at = state.get("completed_at").and_then(|s| s.as_str());

    let elapsed_seconds = if let Some(ca) = completed_at {
        if let Ok(ca_dt) = DateTime::parse_from_rfc3339(ca) {
            elapsed_since(started_at, Some(ca_dt))
        } else {
            elapsed_since(started_at, Some(now))
        }
    } else {
        elapsed_since(started_at, Some(now))
    };

    let queue = state
        .get("queue")
        .and_then(|q| q.as_array())
        .cloned()
        .unwrap_or_default();

    let completed_count = queue
        .iter()
        .filter(|item| item.get("outcome").and_then(|o| o.as_str()) == Some("completed"))
        .count();
    let failed_count = queue
        .iter()
        .filter(|item| item.get("outcome").and_then(|o| o.as_str()) == Some("failed"))
        .count();

    let items: Vec<OrchestrationItem> = queue
        .iter()
        .map(|item| {
            let status = item
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("pending");
            let icon = status_icon(status).to_string();

            let item_started = item.get("started_at").and_then(|s| s.as_str());
            let item_completed = item
                .get("completed_at")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty());

            let item_elapsed = if let (Some(is), Some(ic)) = (item_started, item_completed) {
                if let Ok(ic_dt) = DateTime::parse_from_rfc3339(ic) {
                    format_time(elapsed_since(Some(is), Some(ic_dt)))
                } else {
                    String::new()
                }
            } else if item_started.is_some() && status == "in_progress" {
                format_time(elapsed_since(item_started, Some(now)))
            } else {
                String::new()
            };

            OrchestrationItem {
                icon,
                issue_number: item.get("issue_number").and_then(|n| n.as_i64()),
                title: item
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                elapsed: item_elapsed,
                pr_url: item
                    .get("pr_url")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string()),
                reason: item
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string()),
                status: status.to_string(),
            }
        })
        .collect();

    Some(OrchestrationSummary {
        elapsed: format_time(elapsed_seconds),
        completed_count,
        failed_count,
        total: queue.len(),
        is_running: completed_at.is_none(),
        items,
    })
}

/// Account metrics for TUI header display.
#[derive(Debug, Clone, Serialize)]
pub struct AccountMetrics {
    pub cost_monthly: String,
    pub rl_5h: Option<i64>,
    pub rl_7d: Option<i64>,
    pub stale: bool,
}

/// Load account metrics (monthly cost, rate limits) for TUI header display.
///
/// `home_override` allows tests to specify a fake home directory for rate-limits.json.
pub fn load_account_metrics(repo_root: &Path, home_override: Option<&Path>) -> AccountMetrics {
    // Monthly cost from per-session cost files
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
    let mut total_cost = 0.0f64;
    if cost_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&cost_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(val) = content.trim().parse::<f64>() {
                        total_cost += val;
                    }
                }
            }
        }
    }
    let cost_monthly = format!("{:.2}", total_cost);

    // Rate limits from ~/.claude/rate-limits.json
    let home = match home_override {
        Some(h) => h.to_path_buf(),
        None => std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default(),
    };
    let rl_path = home.join(".claude").join("rate-limits.json");
    let mut rl_5h = None;
    let mut rl_7d = None;
    let mut stale = true;

    if let Ok(metadata) = rl_path.metadata() {
        if let Ok(mtime) = metadata.modified() {
            if let Ok(age) = std::time::SystemTime::now().duration_since(mtime) {
                if age.as_secs() <= STALE_THRESHOLD_SECONDS {
                    if let Ok(content) = std::fs::read_to_string(&rl_path) {
                        if let Ok(data) = serde_json::from_str::<Value>(&content) {
                            // int(data["five_hour_pct"]) — handle null via TypeError parity
                            if let Some(v) = data.get("five_hour_pct") {
                                if let Some(n) = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
                                {
                                    rl_5h = Some(n);
                                }
                            }
                            if let Some(v) = data.get("seven_day_pct") {
                                if let Some(n) = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
                                {
                                    rl_7d = Some(n);
                                }
                            }
                            if rl_5h.is_some() && rl_7d.is_some() {
                                stale = false;
                            }
                        }
                    }
                }
            }
        }
    }

    AccountMetrics {
        cost_monthly,
        rl_5h,
        rl_7d,
        stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Test helper: make_state (mirrors Python conftest.make_state) ---

    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let mut phases = serde_json::Map::new();
        let names_map = phase_config::phase_names();

        for &key in PHASE_ORDER {
            let status = phase_statuses
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, s)| *s)
                .unwrap_or("pending");
            let name = names_map.get(key).cloned().unwrap_or_default();
            phases.insert(
                key.to_string(),
                json!({
                    "name": name,
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": null,
                    "cumulative_seconds": 0,
                    "visit_count": 0,
                }),
            );
        }

        json!({
            "branch": "test-feature",
            "repo": "test/test",
            "pr_number": 1,
            "pr_url": "https://github.com/test/test/pull/1",
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": current_phase,
            "files": {
                "plan": null,
                "dag": null,
                "log": null,
                "state": null,
            },
            "phases": phases,
            "prompt": "",
        })
    }

    // --- step_annotation ---

    #[test]
    fn test_step_annotation_zero_step() {
        assert_eq!(step_annotation(0, 0, ""), "");
    }

    #[test]
    fn test_step_annotation_negative_step() {
        assert_eq!(step_annotation(-1, 5, ""), "");
    }

    #[test]
    fn test_step_annotation_with_total() {
        assert_eq!(step_annotation(3, 11, ""), "step 3 of 11");
    }

    #[test]
    fn test_step_annotation_without_total() {
        assert_eq!(step_annotation(3, 0, ""), "step 3");
    }

    #[test]
    fn test_step_annotation_with_name() {
        assert_eq!(
            step_annotation(5, 11, "pulling main"),
            "pulling main - step 5 of 11"
        );
    }

    #[test]
    fn test_step_annotation_with_name_no_total() {
        assert_eq!(
            step_annotation(3, 0, "creating state"),
            "creating state - step 3"
        );
    }

    // --- step_names ---

    #[test]
    fn test_step_names_start_has_entries() {
        let names = step_names();
        let start = names.get("flow-start").unwrap();
        for key in 3..=11 {
            assert!(
                start.contains_key(&key),
                "missing key {} in flow-start",
                key
            );
        }
        assert_eq!(start.len(), 9);
    }

    #[test]
    fn test_step_names_plan_has_entries() {
        let names = step_names();
        let plan = names.get("flow-plan").unwrap();
        for key in 1..=4 {
            assert!(plan.contains_key(&key), "missing key {} in flow-plan", key);
        }
        assert_eq!(plan.len(), 4);
    }

    #[test]
    fn test_step_names_code_review_has_entries() {
        let names = step_names();
        let cr = names.get("flow-code-review").unwrap();
        for key in 1..=4 {
            assert!(
                cr.contains_key(&key),
                "missing key {} in flow-code-review",
                key
            );
        }
        assert_eq!(cr.len(), 4);
    }

    #[test]
    fn test_step_names_learn_has_entries() {
        let names = step_names();
        let learn = names.get("flow-learn").unwrap();
        for key in 1..=7 {
            assert!(
                learn.contains_key(&key),
                "missing key {} in flow-learn",
                key
            );
        }
        assert_eq!(learn.len(), 7);
    }

    #[test]
    fn test_step_names_complete_has_entries() {
        let names = step_names();
        let complete = names.get("flow-complete").unwrap();
        for key in 1..=12 {
            assert!(
                complete.contains_key(&key),
                "missing key {} in flow-complete",
                key
            );
        }
        assert_eq!(complete.len(), 12);
    }

    // --- status_icon ---

    #[test]
    fn test_status_icon_completed() {
        assert_eq!(status_icon("completed"), "\u{2713}");
    }

    #[test]
    fn test_status_icon_failed() {
        assert_eq!(status_icon("failed"), "\u{2717}");
    }

    #[test]
    fn test_status_icon_in_progress() {
        assert_eq!(status_icon("in_progress"), "\u{25b6}");
    }

    #[test]
    fn test_status_icon_pending() {
        assert_eq!(status_icon("pending"), "\u{00b7}");
    }

    #[test]
    fn test_status_icon_unknown() {
        assert_eq!(status_icon("whatever"), "\u{00b7}");
    }

    // --- phase_timeline ---

    fn pacific(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn test_phase_timeline_all_pending() {
        let state = make_state("flow-start", &[]);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline.len(), PHASE_ORDER.len());
        assert!(timeline.iter().all(|e| e.status == "pending"));
    }

    #[test]
    fn test_phase_timeline_mixed() {
        let now = pacific("2026-01-01T00:02:00-08:00");
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(120);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(480);
        state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");

        let timeline = phase_timeline(&state, Some(now));

        assert_eq!(timeline[0].status, "complete");
        assert_eq!(timeline[0].time, "2m");
        assert_eq!(timeline[0].number, 1);
        assert_eq!(timeline[1].status, "complete");
        assert_eq!(timeline[1].time, "8m");
        assert_eq!(timeline[2].status, "in_progress");
        assert_eq!(timeline[2].name, "Code");
        assert_eq!(timeline[2].time, "2m");
        assert_eq!(timeline[3].status, "pending");
    }

    // --- phase_timeline: Start ---

    #[test]
    fn test_phase_timeline_start_annotation() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["start_step"] = json!(3);
        state["start_steps_total"] = json!(11);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        let start_entry = &timeline[0];
        assert_eq!(start_entry.annotation, "creating state - step 3 of 11");
        assert_eq!(start_entry.name, "Start");
    }

    #[test]
    fn test_phase_timeline_start_step_zero() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[0].annotation, "");
    }

    #[test]
    fn test_phase_timeline_start_no_total() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["start_step"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[0].annotation, "creating state - step 3");
    }

    // --- phase_timeline: Plan ---

    #[test]
    fn test_phase_timeline_plan_annotation() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["plan_step"] = json!(2);
        state["plan_steps_total"] = json!(4);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[1].annotation, "decomposing - step 2 of 4");
    }

    #[test]
    fn test_phase_timeline_plan_step_zero() {
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[1].annotation, "");
    }

    #[test]
    fn test_phase_timeline_plan_no_total() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["plan_step"] = json!(2);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[1].annotation, "decomposing - step 2");
    }

    // --- phase_timeline: Code ---

    #[test]
    fn test_phase_timeline_code_with_task_annotation() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        state["diff_stats"] = json!({"files_changed": 5, "insertions": 127, "deletions": 48});

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        let code_entry = &timeline[2];
        assert!(code_entry.annotation.contains("task 4"));
        assert!(code_entry.annotation.contains("+127"));
        assert!(code_entry.annotation.contains("-48"));
    }

    #[test]
    fn test_phase_timeline_code_first_task_annotation() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_tasks_total"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 1 of 3");
    }

    #[test]
    fn test_phase_timeline_code_with_total() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        state["code_tasks_total"] = json!(8);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(timeline[2].annotation.contains("task 4 of 8"));
    }

    #[test]
    fn test_phase_timeline_code_total_absent_fallback() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 4");
        assert!(!timeline[2].annotation.contains("of"));
    }

    #[test]
    fn test_phase_timeline_code_total_with_diff_stats() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        state["code_tasks_total"] = json!(8);
        state["diff_stats"] = json!({"insertions": 127, "deletions": 48});

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 4 of 8, +127 -48");
    }

    #[test]
    fn test_phase_timeline_code_total_zero_ignored() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        state["code_tasks_total"] = json!(0);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 4");
        assert!(!timeline[2].annotation.contains("of"));
    }

    // --- phase_timeline: Code overflow cap ---

    #[test]
    fn test_phase_timeline_code_task_overflow_capped() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        state["code_tasks_total"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 3 of 3");
    }

    #[test]
    fn test_phase_timeline_code_task_overflow_exceeds_total() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(5);
        state["code_tasks_total"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 3 of 3");
    }

    // --- phase_timeline: Code task name ---

    #[test]
    fn test_phase_timeline_code_with_task_name() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(1);
        state["code_tasks_total"] = json!(3);
        state["code_task_name"] = json!("Update contract tests");

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(
            timeline[2].annotation,
            "Update contract tests - task 2 of 3"
        );
    }

    #[test]
    fn test_phase_timeline_code_task_name_absent() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(1);
        state["code_tasks_total"] = json!(3);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 2 of 3");
    }

    #[test]
    fn test_phase_timeline_code_task_name_with_diff_stats() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(1);
        state["code_tasks_total"] = json!(3);
        state["code_task_name"] = json!("Update contract tests");
        state["diff_stats"] = json!({"insertions": 127, "deletions": 48});

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(
            timeline[2].annotation,
            "Update contract tests - task 2 of 3, +127 -48"
        );
    }

    #[test]
    fn test_phase_timeline_code_task_name_truncated() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(0);
        state["code_tasks_total"] = json!(3);
        state["code_task_name"] =
            json!("Implement the very long task description that exceeds limit");

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        let name_part = timeline[2].annotation.split(" - task ").next().unwrap();
        assert_eq!(name_part.chars().count(), 30);
        assert!(name_part.ends_with("..."));
    }

    #[test]
    fn test_phase_timeline_code_task_name_empty_string() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(1);
        state["code_tasks_total"] = json!(3);
        state["code_task_name"] = json!("");

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[2].annotation, "task 2 of 3");
    }

    // --- phase_timeline: Code Review ---

    #[test]
    fn test_phase_timeline_code_review_step_zero() {
        let state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[3].annotation, "simplifying - step 1 of 4");
    }

    #[test]
    fn test_phase_timeline_code_review_annotation() {
        let mut state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        state["code_review_step"] = json!(2);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[3].annotation, "security review - step 3 of 4");
    }

    #[test]
    fn test_phase_timeline_code_review_complete() {
        let mut state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        state["code_review_step"] = json!(4);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[3].annotation, "");
    }

    #[test]
    fn test_phase_timeline_code_review_step_four() {
        let mut state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        state["code_review_step"] = json!(3);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[3].annotation, "agent reviews - step 4 of 4");
    }

    // --- phase_timeline: step name fallback ---

    #[test]
    fn test_phase_timeline_unknown_step_falls_back() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["start_step"] = json!(1);
        state["start_steps_total"] = json!(11);

        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[0].annotation, "step 1 of 11");
    }

    // --- phase_timeline: Learn ---

    #[test]
    fn test_phase_timeline_learn_annotation() {
        let mut state = make_state(
            "flow-learn",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "in_progress"),
            ],
        );
        state["learn_step"] = json!(4);
        state["learn_steps_total"] = json!(7);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[4].annotation, "committing - step 5 of 7");
    }

    #[test]
    fn test_phase_timeline_learn_step_zero() {
        let mut state = make_state(
            "flow-learn",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "in_progress"),
            ],
        );
        state["learn_steps_total"] = json!(7);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[4].annotation, "gathering sources - step 1 of 7");
    }

    // --- phase_timeline: Complete ---

    #[test]
    fn test_phase_timeline_complete_annotation() {
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "in_progress"),
            ],
        );
        state["complete_step"] = json!(5);
        state["complete_steps_total"] = json!(12);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[5].annotation, "checking GitHub CI - step 5 of 12");
    }

    #[test]
    fn test_phase_timeline_complete_step_zero() {
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "in_progress"),
            ],
        );
        state["complete_steps_total"] = json!(12);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[5].annotation, "");
    }

    #[test]
    fn test_phase_timeline_complete_step_one() {
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "in_progress"),
            ],
        );
        state["complete_step"] = json!(1);
        state["complete_steps_total"] = json!(12);
        let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(timeline[5].annotation, "checking state - step 1 of 12");
    }

    // --- phase_timeline: live elapsed for in-progress ---

    #[test]
    fn test_phase_timeline_in_progress_live_time() {
        let now = pacific("2026-01-01T00:05:00-08:00");
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");

        let timeline = phase_timeline(&state, Some(now));
        let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
        assert_eq!(code_entry.time, "5m");
    }

    #[test]
    fn test_phase_timeline_in_progress_cumulative_plus_live() {
        let now = pacific("2026-01-01T00:03:00-08:00");
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(120);

        let timeline = phase_timeline(&state, Some(now));
        let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
        assert_eq!(code_entry.time, "5m");
    }

    #[test]
    fn test_phase_timeline_in_progress_no_session_started() {
        let now = pacific("2026-01-01T00:05:00-08:00");
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-code"]["session_started_at"] = json!(null);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(60);

        let timeline = phase_timeline(&state, Some(now));
        let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
        assert_eq!(code_entry.time, "1m");
    }

    // --- parse_log_entries ---

    #[test]
    fn test_parse_log_entries_basic() {
        let log = "2026-01-01T10:15:00-08:00 [Phase 1] git worktree add (exit 0)\n\
                   2026-01-01T10:20:00-08:00 [Phase 2] Plan approved\n";
        let entries = parse_log_entries(log, 20);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].time, "10:15");
        assert_eq!(entries[0].message, "[Phase 1] git worktree add (exit 0)");
        assert_eq!(entries[1].time, "10:20");
    }

    #[test]
    fn test_parse_log_entries_limit() {
        let lines: Vec<String> = (0..30)
            .map(|i| format!("2026-01-01T10:{:02}:00-08:00 entry {}", i, i))
            .collect();
        let log = lines.join("\n");
        let entries = parse_log_entries(&log, 5);
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].message, "entry 25");
        assert_eq!(entries[4].message, "entry 29");
    }

    #[test]
    fn test_parse_log_entries_empty() {
        let entries = parse_log_entries("", 20);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_parse_log_entries_malformed_lines() {
        let log = "2026-01-01T10:15:00-08:00 valid entry\n\
                   this line has no timestamp\n\
                   2026-01-01T10:20:00-08:00 another valid entry\n";
        let entries = parse_log_entries(log, 20);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "valid entry");
        assert_eq!(entries[1].message, "another valid entry");
    }

    #[test]
    fn test_parse_log_entries_blank_lines() {
        let log = "2026-01-01T10:15:00-08:00 first entry\n\n\
                   2026-01-01T10:20:00-08:00 second entry\n";
        let entries = parse_log_entries(log, 20);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_log_entries_invalid_timestamp() {
        let log = "9999-99-99T99:99:99-08:00 bad timestamp\n";
        let entries = parse_log_entries(log, 20);
        assert_eq!(entries.len(), 0);
    }

    // --- flow_summary ---

    #[test]
    fn test_flow_summary_basic() {
        let now = pacific("2026-01-01T01:00:00-08:00");
        let state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        let summary = flow_summary(&state, Some(now));
        assert_eq!(summary.feature, "Test Feature");
        assert_eq!(summary.branch, "test-feature");
        assert_eq!(summary.worktree, ".worktrees/test-feature");
        assert_eq!(summary.pr_number, Some(1));
        assert_eq!(
            summary.pr_url.as_deref(),
            Some("https://github.com/test/test/pull/1")
        );
        assert_eq!(summary.phase_number, 3);
        assert_eq!(summary.phase_name, "Code");
    }

    #[test]
    fn test_flow_summary_elapsed_time() {
        let now = pacific("2026-01-01T00:42:00-08:00");
        let mut state = make_state("flow-start", &[]);
        state["started_at"] = json!("2026-01-01T00:00:00-08:00");
        let summary = flow_summary(&state, Some(now));
        assert_eq!(summary.elapsed, "42m");
    }

    #[test]
    fn test_flow_summary_code_task_present() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(3);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.code_task, 3);
    }

    #[test]
    fn test_flow_summary_code_task_absent() {
        let state = make_state("flow-start", &[]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.code_task, 0);
    }

    #[test]
    fn test_flow_summary_diff_stats_present() {
        let mut state = make_state("flow-start", &[]);
        state["diff_stats"] = json!({"files_changed": 5, "insertions": 100, "deletions": 20});
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.diff_stats.is_some());
    }

    #[test]
    fn test_flow_summary_diff_stats_absent() {
        let state = make_state("flow-start", &[]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.diff_stats.is_none());
    }

    #[test]
    fn test_flow_summary_notes_count() {
        let mut state = make_state("flow-start", &[]);
        state["notes"] = json!([{"text": "note1"}, {"text": "note2"}]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.notes_count, 2);
    }

    #[test]
    fn test_flow_summary_issues_count() {
        let mut state = make_state("flow-start", &[]);
        state["issues_filed"] = json!([{"url": "http://example.com/1"}]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.issues_count, 1);
    }

    #[test]
    fn test_flow_summary_no_notes_or_issues() {
        let state = make_state("flow-start", &[]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.notes_count, 0);
        assert_eq!(summary.issues_count, 0);
    }

    #[test]
    fn test_flow_summary_issues_populated() {
        let mut state = make_state("flow-start", &[]);
        state["issues_filed"] = json!([
            {
                "label": "Tech Debt",
                "title": "Extract helper for date parsing",
                "url": "https://github.com/test/test/issues/42",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
            },
            {
                "label": "Flaky Test",
                "title": "test_timeout flakes on CI",
                "url": "https://github.com/test/test/issues/55",
                "phase": "flow-code",
                "phase_name": "Code",
            },
        ]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.issues.len(), 2);
        assert_eq!(summary.issues[0].label, "Tech Debt");
        assert_eq!(summary.issues[0].title, "Extract helper for date parsing");
        assert_eq!(
            summary.issues[0].url,
            "https://github.com/test/test/issues/42"
        );
        assert_eq!(summary.issues[0].ref_str, "#42");
        assert_eq!(summary.issues[0].phase_name, "Code Review");
        assert_eq!(summary.issues[1].ref_str, "#55");
    }

    #[test]
    fn test_flow_summary_issues_empty() {
        let mut state = make_state("flow-start", &[]);
        state["issues_filed"] = json!([]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.issues.is_empty());
    }

    #[test]
    fn test_flow_summary_issues_url_fallback() {
        let mut state = make_state("flow-start", &[]);
        state["issues_filed"] = json!([{
            "label": "Flow",
            "title": "Process gap",
            "url": "https://example.com/custom/path",
            "phase": "flow-learn",
            "phase_name": "Learn",
        }]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.issues[0].ref_str, "https://example.com/custom/path");
    }

    #[test]
    fn test_flow_summary_blocked_true() {
        let mut state = make_state("flow-code", &[("flow-code", "in_progress")]);
        state["_blocked"] = json!("2026-01-01T10:00:00-08:00");
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.blocked);
    }

    #[test]
    fn test_flow_summary_blocked_false() {
        let state = make_state("flow-code", &[("flow-code", "in_progress")]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(!summary.blocked);
    }

    #[test]
    fn test_flow_summary_blocked_empty_string() {
        let mut state = make_state("flow-code", &[("flow-code", "in_progress")]);
        state["_blocked"] = json!("");
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(!summary.blocked);
    }

    #[test]
    fn test_flow_summary_issue_numbers() {
        let mut state = make_state("flow-start", &[]);
        state["prompt"] = json!("work on #83 and #89");
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.issue_numbers.contains(&83));
        assert!(summary.issue_numbers.contains(&89));
    }

    #[test]
    fn test_flow_summary_plan_path_from_files() {
        let mut state = make_state("flow-start", &[]);
        state["files"]["plan"] = json!(".flow-states/test-feature-plan.md");
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(
            summary.plan_path.as_deref(),
            Some(".flow-states/test-feature-plan.md")
        );
    }

    #[test]
    fn test_flow_summary_plan_path_fallback_plan_file() {
        let mut state = make_state("flow-start", &[]);
        state["files"]["plan"] = json!(null);
        state["plan_file"] = json!(".flow-states/test-feature-plan.md");
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(
            summary.plan_path.as_deref(),
            Some(".flow-states/test-feature-plan.md")
        );
    }

    #[test]
    fn test_flow_summary_plan_path_absent() {
        let state = make_state("flow-start", &[]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert!(summary.plan_path.is_none());
    }

    #[test]
    fn test_flow_summary_phase_elapsed() {
        let now = pacific("2026-01-01T00:05:00-08:00");
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");
        let summary = flow_summary(&state, Some(now));
        assert_eq!(summary.phase_elapsed, "5m");
    }

    #[test]
    fn test_flow_summary_phase_elapsed_no_in_progress() {
        let now = pacific("2026-01-01T01:00:00-08:00");
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "pending")],
        );
        let summary = flow_summary(&state, Some(now));
        assert_eq!(summary.phase_elapsed, "");
    }

    #[test]
    fn test_flow_summary_annotation_code_phase() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["code_task"] = json!(2);
        state["code_tasks_total"] = json!(5);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.annotation, "task 3 of 5");
    }

    #[test]
    fn test_flow_summary_annotation_no_step_set() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.annotation, "");
    }

    #[test]
    fn test_flow_summary_annotation_start_phase() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["start_step"] = json!(5);
        state["start_steps_total"] = json!(11);
        let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
        assert_eq!(summary.annotation, "pulling main - step 5 of 11");
    }

    // --- load_all_flows ---

    #[test]
    fn test_load_all_flows_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".flow-states")).unwrap();
        let result = load_all_flows(dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_all_flows_single() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        let state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        std::fs::write(
            state_dir.join("test-feature.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
        let result = load_all_flows(dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].branch, "test-feature");
    }

    #[test]
    fn test_load_all_flows_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        for name in ["charlie-feature", "alpha-feature", "bravo-feature"] {
            let mut state = make_state("flow-start", &[]);
            state["branch"] = json!(name);
            std::fs::write(
                state_dir.join(format!("{}.json", name)),
                serde_json::to_string(&state).unwrap(),
            )
            .unwrap();
        }
        let result = load_all_flows(dir.path());
        assert_eq!(result.len(), 3);
        let names: Vec<&str> = result.iter().map(|f| f.branch.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha-feature", "bravo-feature", "charlie-feature"]
        );
    }

    #[test]
    fn test_load_all_flows_skips_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        let state = make_state("flow-start", &[]);
        std::fs::write(
            state_dir.join("good-feature.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
        std::fs::write(state_dir.join("bad-feature.json"), "{invalid json").unwrap();
        let result = load_all_flows(dir.path());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_load_all_flows_skips_phases_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        let mut state = make_state("flow-start", &[]);
        state["branch"] = json!("my-feature");
        std::fs::write(
            state_dir.join("my-feature.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
        std::fs::write(state_dir.join("my-feature-phases.json"), r#"{"order": []}"#).unwrap();
        let result = load_all_flows(dir.path());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_load_all_flows_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_all_flows(dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_all_flows_skips_json_without_branch() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        std::fs::write(state_dir.join("no-branch.json"), r#"{"some": "data"}"#).unwrap();
        let state = make_state("flow-start", &[]);
        std::fs::write(
            state_dir.join("real-feature.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
        let result = load_all_flows(dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].branch, "test-feature");
    }

    // --- load_orchestration ---

    #[test]
    fn test_load_orchestration_no_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".flow-states")).unwrap();
        assert!(load_orchestration(dir.path()).is_none());
    }

    #[test]
    fn test_load_orchestration_with_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [{"issue_number": 42, "title": "Add PDF export", "status": "pending"}],
        });
        std::fs::write(
            state_dir.join("orchestrate.json"),
            serde_json::to_string(&orch).unwrap(),
        )
        .unwrap();
        let result = load_orchestration(dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            r.get("started_at").unwrap().as_str().unwrap(),
            "2026-03-20T22:00:00-07:00"
        );
    }

    #[test]
    fn test_load_orchestration_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();
        std::fs::write(state_dir.join("orchestrate.json"), "{corrupt json").unwrap();
        assert!(load_orchestration(dir.path()).is_none());
    }

    #[test]
    fn test_load_orchestration_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_orchestration(dir.path()).is_none());
    }

    // --- orchestration_summary ---

    #[test]
    fn test_orchestration_summary_no_state() {
        assert!(orchestration_summary(None, None).is_none());
    }

    #[test]
    fn test_orchestration_summary_basic() {
        let now = pacific("2026-03-21T00:00:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [
                {
                    "issue_number": 42, "title": "Add PDF export",
                    "status": "completed", "outcome": "completed",
                    "started_at": "2026-03-20T22:00:00-07:00",
                    "completed_at": "2026-03-20T23:24:00-07:00",
                    "pr_url": "https://github.com/test/test/pull/58",
                },
                {
                    "issue_number": 43, "title": "Fix login timeout",
                    "status": "pending", "outcome": null,
                    "started_at": null, "completed_at": null,
                },
            ],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.failed_count, 0);
        assert!(summary.is_running);
        assert_eq!(summary.items[0].icon, "\u{2713}");
        assert_eq!(summary.items[0].issue_number, Some(42));
        assert_eq!(summary.items[1].icon, "\u{00b7}");
    }

    #[test]
    fn test_orchestration_summary_with_completed_and_failed() {
        let now = pacific("2026-03-21T02:00:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [
                {"issue_number": 42, "title": "A", "status": "completed", "outcome": "completed",
                 "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
                {"issue_number": 43, "title": "B", "status": "failed", "outcome": "failed",
                 "started_at": "2026-03-20T23:00:00-07:00", "completed_at": "2026-03-21T00:00:00-07:00",
                 "reason": "CI failed after 3 attempts"},
                {"issue_number": 44, "title": "C", "status": "pending", "outcome": null},
            ],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.items[1].icon, "\u{2717}");
        assert_eq!(
            summary.items[1].reason.as_deref(),
            Some("CI failed after 3 attempts")
        );
    }

    #[test]
    fn test_orchestration_summary_in_progress_elapsed() {
        let now = pacific("2026-03-21T00:38:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [
                {"issue_number": 45, "title": "Update hooks",
                 "status": "in_progress",
                 "started_at": "2026-03-21T00:00:00-07:00"},
            ],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert_eq!(summary.items[0].icon, "\u{25b6}");
        assert_eq!(summary.items[0].elapsed, "38m");
    }

    #[test]
    fn test_orchestration_summary_no_queue() {
        let now = pacific("2026-03-21T00:00:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert_eq!(summary.total, 0);
        assert!(summary.items.is_empty());
        assert!(summary.is_running);
    }

    #[test]
    fn test_orchestration_summary_not_running() {
        let now = pacific("2026-03-21T06:00:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": "2026-03-20T23:00:00-07:00",
            "queue": [
                {"issue_number": 42, "title": "Done", "status": "completed", "outcome": "completed",
                 "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
            ],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert!(!summary.is_running);
        assert_eq!(summary.elapsed, "1h 0m");
    }

    #[test]
    fn test_queue_item_display_icons() {
        let now = pacific("2026-03-21T00:00:00-07:00");
        let orch = json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [
                {"issue_number": 1, "title": "A", "status": "completed", "outcome": "completed",
                 "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
                {"issue_number": 2, "title": "B", "status": "failed", "outcome": "failed",
                 "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
                {"issue_number": 3, "title": "C", "status": "in_progress",
                 "started_at": "2026-03-20T23:00:00-07:00"},
                {"issue_number": 4, "title": "D", "status": "pending"},
            ],
        });
        let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
        assert_eq!(summary.items[0].icon, "\u{2713}");
        assert_eq!(summary.items[1].icon, "\u{2717}");
        assert_eq!(summary.items[2].icon, "\u{25b6}");
        assert_eq!(summary.items[3].icon, "\u{00b7}");
    }

    // --- load_account_metrics ---

    #[test]
    fn test_load_account_metrics_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let year_month = chrono::Local::now().format("%Y-%m").to_string();
        let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
        std::fs::create_dir_all(&cost_dir).unwrap();
        std::fs::write(cost_dir.join("session-a"), "1.50").unwrap();
        std::fs::write(cost_dir.join("session-b"), "2.75").unwrap();

        let home_dir = dir.path().join("home");
        let claude_dir = home_dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("rate-limits.json"),
            r#"{"five_hour_pct": 45, "seven_day_pct": 32}"#,
        )
        .unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert_eq!(result.cost_monthly, "4.25");
        assert_eq!(result.rl_5h, Some(45));
        assert_eq!(result.rl_7d, Some(32));
        assert!(!result.stale);
    }

    #[test]
    fn test_load_account_metrics_no_cost_directory() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let home_dir = dir.path().join("home");
        let claude_dir = home_dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("rate-limits.json"),
            r#"{"five_hour_pct": 10, "seven_day_pct": 20}"#,
        )
        .unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert_eq!(result.cost_monthly, "0.00");
    }

    #[test]
    fn test_load_account_metrics_no_rate_limits_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let home_dir = dir.path().join("home");
        std::fs::create_dir(&home_dir).unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert!(result.stale);
        assert!(result.rl_5h.is_none());
        assert!(result.rl_7d.is_none());
    }

    #[test]
    fn test_load_account_metrics_stale_rate_limits() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let home_dir = dir.path().join("home");
        let claude_dir = home_dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let rl_path = claude_dir.join("rate-limits.json");
        std::fs::write(&rl_path, r#"{"five_hour_pct": 55, "seven_day_pct": 40}"#).unwrap();
        // Set mtime to 15 minutes ago
        let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(900);
        filetime::set_file_mtime(&rl_path, filetime::FileTime::from_system_time(old_time)).unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert!(result.stale);
        assert!(result.rl_5h.is_none());
        assert!(result.rl_7d.is_none());
    }

    #[test]
    fn test_load_account_metrics_malformed_cost_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let year_month = chrono::Local::now().format("%Y-%m").to_string();
        let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
        std::fs::create_dir_all(&cost_dir).unwrap();
        std::fs::write(cost_dir.join("good-session"), "3.00").unwrap();
        std::fs::write(cost_dir.join("bad-session"), "not-a-number").unwrap();

        let home_dir = dir.path().join("home");
        std::fs::create_dir(&home_dir).unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert_eq!(result.cost_monthly, "3.00");
    }

    #[test]
    fn test_load_account_metrics_malformed_rate_limits() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let home_dir = dir.path().join("home");
        let claude_dir = home_dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("rate-limits.json"), "{invalid json").unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert!(result.stale);
        assert!(result.rl_5h.is_none());
        assert!(result.rl_7d.is_none());
    }

    #[test]
    fn test_load_all_flows_sorted_by_phase_then_feature() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();

        // Flow in Code phase (phase 3) — branch "alpha" sorts first alphabetically
        let mut code_state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        code_state["branch"] = json!("alpha-feature");
        std::fs::write(
            state_dir.join("alpha-feature.json"),
            serde_json::to_string(&code_state).unwrap(),
        )
        .unwrap();

        // Flow in Start phase (phase 1) — branch "beta" sorts second alphabetically
        let mut start_state = make_state("flow-start", &[("flow-start", "in_progress")]);
        start_state["branch"] = json!("beta-feature");
        std::fs::write(
            state_dir.join("beta-feature.json"),
            serde_json::to_string(&start_state).unwrap(),
        )
        .unwrap();

        // Flow in Plan phase (phase 2)
        let mut plan_state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        plan_state["branch"] = json!("gamma-feature");
        std::fs::write(
            state_dir.join("gamma-feature.json"),
            serde_json::to_string(&plan_state).unwrap(),
        )
        .unwrap();

        // Second flow in Start phase (phase 1) — tiebreaker: "delta" > "beta" alphabetically
        let mut start_state2 = make_state("flow-start", &[("flow-start", "in_progress")]);
        start_state2["branch"] = json!("delta-feature");
        std::fs::write(
            state_dir.join("delta-feature.json"),
            serde_json::to_string(&start_state2).unwrap(),
        )
        .unwrap();

        let flows = load_all_flows(dir.path());

        assert_eq!(flows.len(), 4);
        // Phase 1 (Start) first, alphabetical tiebreaker: Beta < Delta
        assert_eq!(flows[0].branch, "beta-feature");
        assert_eq!(flows[0].phase_number, 1);
        assert_eq!(flows[1].branch, "delta-feature");
        assert_eq!(flows[1].phase_number, 1);
        // Phase 2 (Plan) next
        assert_eq!(flows[2].branch, "gamma-feature");
        assert_eq!(flows[2].phase_number, 2);
        // Phase 3 (Code) last
        assert_eq!(flows[3].branch, "alpha-feature");
        assert_eq!(flows[3].phase_number, 3);
    }

    #[test]
    fn test_load_all_flows_unknown_phase_sorts_last() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir(&state_dir).unwrap();

        // Flow with recognized phase (Start, phase 1)
        let mut start_state = make_state("flow-start", &[("flow-start", "in_progress")]);
        start_state["branch"] = json!("known-feature");
        std::fs::write(
            state_dir.join("known-feature.json"),
            serde_json::to_string(&start_state).unwrap(),
        )
        .unwrap();

        // Flow with unrecognized phase
        let mut unknown_state = make_state("flow-nonexistent", &[]);
        unknown_state["branch"] = json!("unknown-feature");
        std::fs::write(
            state_dir.join("unknown-feature.json"),
            serde_json::to_string(&unknown_state).unwrap(),
        )
        .unwrap();

        let flows = load_all_flows(dir.path());

        assert_eq!(flows.len(), 2);
        // Known phase sorts first; unknown phase sorts last
        assert_eq!(flows[0].branch, "known-feature");
        assert_eq!(flows[0].phase_number, 1);
        assert_eq!(flows[1].branch, "unknown-feature");
        assert_eq!(flows[1].phase_number, usize::MAX);
    }

    #[test]
    fn test_load_account_metrics_null_rate_limit_values() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir(&repo_root).unwrap();

        let home_dir = dir.path().join("home");
        let claude_dir = home_dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("rate-limits.json"),
            r#"{"five_hour_pct": null, "seven_day_pct": null}"#,
        )
        .unwrap();

        let result = load_account_metrics(&repo_root, Some(&home_dir));
        assert!(result.stale);
        assert!(result.rl_5h.is_none());
        assert!(result.rl_7d.is_none());
    }
}
