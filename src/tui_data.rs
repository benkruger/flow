//! Pure data layer for the FLOW interactive TUI.
//!
//! Reads state files, computes display structs (flow summaries, phase timelines,
//! log entries). No curses dependency — fully testable with make_state() fixture.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use serde_json::Value;

use crate::flow_paths::FlowStatesDir;
use crate::phase_config::{self, PHASE_ORDER};
use crate::utils::{
    derive_feature, derive_worktree, elapsed_since, extract_issue_numbers, format_time,
    short_issue_ref, tolerant_i64_opt,
};

/// Static mapping of (phase_key, display_step_number) → short step name.
///
/// Display step number is what the user sees in the annotation.
/// Source: skill SKILL.md step headings (## Step N — Name).
pub fn step_names() -> HashMap<&'static str, HashMap<i64, &'static str>> {
    let mut map = HashMap::new();

    let mut start = HashMap::new();
    start.insert(1, "initializing");
    start.insert(2, "CI gate");
    start.insert(3, "creating workspace");
    start.insert(4, "entering worktree");
    start.insert(5, "finalizing");
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
    complete.insert(1, "running checks");
    complete.insert(2, "local CI");
    complete.insert(3, "GitHub CI");
    complete.insert(4, "confirming");
    complete.insert(5, "merging PR");
    complete.insert(6, "finalizing");
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
        // Every PHASE_ORDER key is present in `phase_names()` — the
        // contract test `phase_order_keys_all_present_in_phase_names`
        // locks the invariant. The expect arm is unreachable in
        // production; it fails loudly if the two tables drift.
        let name = names_map
            .get(key)
            .cloned()
            .expect("PHASE_ORDER key must be in phase_names");

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
                // Truncate by char count so multi-byte UTF-8 names
                // (emoji, CJK) cannot land mid-codepoint and panic the
                // formatter on display.
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
        } else {
            // PHASE_ORDER guarantees the only remaining key here is
            // "flow-complete" (the prior arms cover every other phase).
            // Collapsing flow-complete into the final else removes a
            // dead arm and keeps coverage at 100% without an
            // unreachable defensive branch.
            debug_assert_eq!(key, "flow-complete");
            let sn = all_step_names
                .get("flow-complete")
                .and_then(|m| m.get(&complete_step))
                .copied()
                .unwrap_or("");
            step_annotation(complete_step, complete_steps_total, sn)
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

/// A single row in the per-phase token cost table.
///
/// One row per `PHASE_ORDER` entry — the row exists even when the
/// phase carries no snapshot data so the TUI renders a stable layout.
/// Token / cost / reset fields are computed via
/// `window_deltas::phase_delta`; rows for phases with no enter
/// snapshot fall back to zero values.
#[derive(Debug, Clone, Serialize)]
pub struct PhaseTokenRow {
    pub phase_key: String,
    pub phase_name: String,
    pub phase_number: usize,
    pub status: String,
    pub tokens: i64,
    pub cost_usd: f64,
    pub window_reset_observed: bool,
    pub in_progress: bool,
}

/// Build a per-phase token cost table for the TUI flow detail panel.
///
/// Returns one row per `PHASE_ORDER` entry. Phases without snapshots
/// produce zero-valued rows so the layout is stable. State that fails
/// the `FlowState` parse (legacy or corrupted) renders the per-phase
/// row scaffold with zero token data — the TUI still gets a layout
/// to render. Returns an empty Vec when `phases` is missing or
/// non-object.
pub fn phase_token_table(state: &Value) -> Vec<PhaseTokenRow> {
    let phases = match state.get("phases").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return vec![],
    };
    let names_map = phase_config::phase_names();
    let numbers_map = phase_config::phase_numbers();

    // Parse the state as FlowState for delta computation. Fail-open:
    // rows still render when parse fails (older state files, missing
    // required fields), just with zero token data.
    let flow_state: Option<crate::state::FlowState> = serde_json::from_value(state.clone()).ok();

    let mut rows = Vec::new();
    for &key in PHASE_ORDER {
        let phase = match phases.get(key) {
            Some(p) => p,
            None => continue,
        };
        let status = phase
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("pending")
            .to_string();
        let phase_name = names_map.get(key).cloned().unwrap_or_default();
        let phase_number = numbers_map.get(key).copied().unwrap_or(0);
        let in_progress = status == "in_progress";

        let phase_enum: Option<crate::state::Phase> =
            serde_json::from_value(serde_json::json!(key)).ok();
        let (tokens, cost_usd, window_reset_observed) = flow_state
            .as_ref()
            .zip(phase_enum.as_ref())
            .and_then(|(fs, pe)| fs.phases.get(pe))
            .and_then(crate::window_deltas::phase_delta)
            .map(|report| {
                let total = report
                    .input_tokens_delta
                    .saturating_add(report.output_tokens_delta)
                    .saturating_add(report.cache_creation_tokens_delta)
                    .saturating_add(report.cache_read_tokens_delta);
                (total, report.cost_delta_usd, report.window_reset_observed)
            })
            .unwrap_or((0, 0.0, false));

        rows.push(PhaseTokenRow {
            phase_key: key.to_string(),
            phase_name,
            phase_number,
            status,
            tokens,
            cost_usd,
            window_reset_observed,
            in_progress,
        });
    }
    rows
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
    /// Serializes to JSON as "ref" — the field name the TUI display
    /// layer expects in its issue summary schema. The Rust field is
    /// named `ref_str` because `ref` is a reserved keyword.
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
            // Treat the `_blocked` field as set when it is a non-empty
            // string, a true bool, or any non-null compound value.
            // Empty strings and null are explicitly "not blocked".
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

/// Read every `.flow-states/<branch>/state.json` file and return flow
/// summaries sorted by phase number (ascending), then by feature name
/// (alphabetical) as a tiebreaker.
///
/// Discovery iterates subdirectories of `.flow-states/` and selects
/// each one that contains a readable `state.json` whose JSON has a
/// `branch` field. Subdirectories without `state.json` and regular
/// files at the root of `.flow-states/` (such as `orchestrate.json`,
/// the start lock, or stale flat-layout artifacts left by older
/// binaries) are skipped naturally.
pub fn load_all_flows(root: &Path) -> Vec<FlowSummary> {
    let state_dir = FlowStatesDir::new(root).path().to_path_buf();
    if !state_dir.is_dir() {
        return vec![];
    }

    let mut subdirs: Vec<_> = match std::fs::read_dir(&state_dir) {
        Ok(iter) => iter
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .collect(),
        Err(_) => return vec![],
    };
    subdirs.sort_by_key(|e| e.file_name());

    let mut flows = Vec::new();
    for entry in subdirs {
        let state_path = entry.path().join("state.json");
        if !state_path.is_file() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&state_path) {
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
    let state_dir = FlowStatesDir::new(root).path().to_path_buf();
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

    // Rate-limits freshness gate. `metadata.modified()` and
    // `SystemTime::now().duration_since(mtime)` are both fallible at
    // the type level but unreachable in practice on macOS/Linux:
    // APFS/EXT filesystems always populate mtime, and a future-mtime
    // (clock skew) triggering `duration_since` Err is exotic enough
    // that we collapse it into the staleness branch via `unwrap_or`.
    // That keeps the chain linear and ensures coverage tracks only
    // branches that are reachable from a portable test.
    if let Ok(metadata) = rl_path.metadata() {
        let mtime = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let age = std::time::SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(std::time::Duration::MAX);
        if age.as_secs() <= STALE_THRESHOLD_SECONDS {
            if let Ok(content) = std::fs::read_to_string(&rl_path) {
                if let Ok(data) = serde_json::from_str::<Value>(&content) {
                    rl_5h = data.get("five_hour_pct").and_then(tolerant_i64_opt);
                    rl_7d = data.get("seven_day_pct").and_then(tolerant_i64_opt);
                    if rl_5h.is_some() && rl_7d.is_some() {
                        stale = false;
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

/// Driver for the `bin/flow tui-data` subcommand.
///
/// Returns `Result<(stdout_value, code), (stderr_text, code)>`:
///
/// - `Ok((value, 0))` — JSON to write to stdout for one of the three
///   flag branches (`--load-all-flows`, `--load-orchestration`,
///   `--load-account-metrics`). The `orchestration` no-state case
///   returns `Value::Null` so the caller prints the string `"null"`.
/// - `Err((msg, 1))` — none of the three flags was set. The caller
///   writes the message to stderr and exits 1, matching the
///   pre-extraction contract.
///
/// Tests supply `root` as a fixture TempDir and stage state files
/// under `.flow-states/` before calling.
pub fn run_impl_main(
    load_all: bool,
    load_orch: bool,
    load_metrics: bool,
    root: &Path,
) -> Result<(Value, i32), (String, i32)> {
    if load_all {
        let flows = load_all_flows(root);
        return Ok((
            serde_json::to_value(&flows).expect("flow summaries serialize"),
            0,
        ));
    }
    if load_orch {
        return match load_orchestration(root) {
            Some(state) => {
                let summary = orchestration_summary(Some(&state), None);
                let result = serde_json::json!({
                    "state": state,
                    "summary": summary,
                });
                Ok((result, 0))
            }
            None => Ok((Value::Null, 0)),
        };
    }
    if load_metrics {
        let metrics = load_account_metrics(root, None);
        return Ok((
            serde_json::to_value(&metrics).expect("metrics serialize"),
            0,
        ));
    }
    Err((
        "tui-data: specify one of --load-all-flows, --load-orchestration, --load-account-metrics"
            .to_string(),
        1,
    ))
}
