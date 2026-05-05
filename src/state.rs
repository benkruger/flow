use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The six FLOW phases, serialized as hyphenated keys (e.g. "flow-start").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    #[serde(rename = "flow-start")]
    FlowStart,
    #[serde(rename = "flow-plan")]
    FlowPlan,
    #[serde(rename = "flow-code")]
    FlowCode,
    #[serde(rename = "flow-code-review")]
    FlowCodeReview,
    #[serde(rename = "flow-learn")]
    FlowLearn,
    #[serde(rename = "flow-complete")]
    FlowComplete,
}

/// Phase lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "complete")]
    Complete,
}

/// Per-phase state tracking.
///
/// `window_at_enter`, `window_at_complete`, and `step_snapshots` capture
/// account-wide token / cost / rate-limit observations at every
/// state-mutating phase transition. The values are populated by the
/// `window_snapshot::capture` helper invoked from `phase_enter`,
/// `phase_finalize`, `phase_transition`, and `set_timestamp` (when the
/// mutated field names a step counter). Readers in `window_deltas`
/// derive per-phase deltas, by-model rollups, and reset detection
/// from these raw snapshots — all numeric snapshot fields are
/// `Option<_>` so missing inputs (no rate-limits file, missing
/// transcript, no cost file) surface as `None` rather than panics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseState {
    pub name: String,
    pub status: PhaseStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub session_started_at: Option<String>,
    pub cumulative_seconds: i64,
    pub visit_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_at_enter: Option<WindowSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_at_complete: Option<WindowSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_snapshots: Vec<StepSnapshot>,
}

/// Per-model token counters extracted from the session transcript.
///
/// Sessions can mix models (e.g. an Opus turn followed by a Sonnet
/// turn after `/model` switches). Each `assistant` message in the
/// transcript names its model in `message.model`; capture sums the
/// usage fields per model into one entry of `WindowSnapshot.by_model`.
/// All fields are non-optional `i64` because the entry only exists
/// when at least one assistant message contributed to its model
/// (zero is a meaningful value within a populated entry).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelTokens {
    pub input: i64,
    pub output: i64,
    pub cache_create: i64,
    pub cache_read: i64,
}

/// Account-wide window observation captured at a state transition.
///
/// Every numeric field is `Option<i64>` / `Option<f64>` so that a
/// missing or unreadable input source (rate-limits file, transcript,
/// cost file) leaves the corresponding field as `None` rather than
/// failing the capture. `captured_at` is always populated because the
/// snapshot is constructed at a known wall-clock moment.
///
/// Stored raw — never as deltas. Readers in `window_deltas` compute
/// deltas at read time and detect window resets (`five_hour_pct`
/// going down between snapshots) by inspecting the raw values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub captured_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour_pct: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day_pct: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_output_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cache_creation_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cache_read_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub by_model: IndexMap<String, ModelTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_at_last_turn_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_pct: Option<f64>,
}

/// A `WindowSnapshot` captured at a step-counter boundary.
///
/// Appended to `PhaseState.step_snapshots[]` by `set_timestamp` when
/// the mutated field is one of the five named step counters
/// (`plan_step`, `code_task`, `code_review_step`, `learn_step`,
/// `complete_step`). `step` records the counter value and `field`
/// records which counter; the snapshot fields are flattened into the
/// outer JSON so each entry is one flat object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepSnapshot {
    pub step: i64,
    pub field: String,
    #[serde(flatten)]
    pub snapshot: WindowSnapshot,
}

/// Artifact file paths (relative to project root).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateFiles {
    pub plan: Option<String>,
    pub dag: Option<String>,
    pub log: String,
    pub state: String,
}

/// A correction or observation captured via /flow-note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub phase: String,
    pub phase_name: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub note_type: String,
    pub note: String,
}

/// A phase entry event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub from: Option<String>,
    pub to: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A GitHub issue filed during the feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueFiled {
    pub label: String,
    pub title: String,
    pub url: String,
    pub phase: String,
    pub phase_name: String,
    pub timestamp: String,
}

/// API error context from the last StopFailure event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailureInfo {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    pub timestamp: String,
}

/// Per-skill autonomy config — either a simple string or a detailed map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SkillConfig {
    Simple(String),
    Detailed(IndexMap<String, String>),
}

/// A Slack notification sent during the feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlackNotification {
    pub phase: String,
    pub phase_name: String,
    pub ts: String,
    pub thread_ts: String,
    pub message_preview: String,
    pub timestamp: String,
}

/// The complete FLOW state file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowState {
    pub schema_version: i64,
    pub branch: String,
    /// Relative path inside the worktree where the agent should operate.
    ///
    /// Empty string means the agent operates at the worktree root (the
    /// common case). When non-empty (e.g. `"api"` for a mono-repo flow
    /// started inside `api/`), `start_workspace` cds the agent into
    /// `<worktree>/<relative_cwd>` and every `bin/flow` subcommand
    /// enforces that cwd against this value via `cwd_scope::enforce`.
    /// Captured by `start_init` from `cwd.strip_prefix(project_root())`.
    #[serde(default)]
    pub relative_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    pub started_at: String,
    pub current_phase: String,
    pub files: StateFiles,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_tty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub phases: IndexMap<Phase, PhaseState>,
    #[serde(default)]
    pub phase_transitions: Vec<PhaseTransition>,

    // Legacy fields — superseded by files.plan and files.dag
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dag_file: Option<String>,

    // Per-skill autonomy settings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<IndexMap<String, SkillConfig>>,

    // Issues filed during the feature
    #[serde(default)]
    pub issues_filed: Vec<IssueFiled>,

    // Slack integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack_thread_ts: Option<String>,
    #[serde(default)]
    pub slack_notifications: Vec<SlackNotification>,

    // Start phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_step: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_steps_total: Option<i64>,

    // Plan phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_step: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_steps_total: Option<i64>,

    // Code phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_task: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_tasks_total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_task_name: Option<String>,

    // Code Review phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_review_step: Option<i64>,

    // Learn phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learn_step: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learn_steps_total: Option<i64>,

    // Complete phase TUI progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_step: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_steps_total: Option<i64>,

    // Transient fields (underscore-prefixed in JSON)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "_auto_continue"
    )]
    pub auto_continue: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "_continue_pending"
    )]
    pub continue_pending: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "_continue_context"
    )]
    pub continue_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_blocked")]
    pub blocked: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "_last_failure"
    )]
    pub last_failure: Option<FailureInfo>,

    // Compaction fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_count: Option<i64>,

    // Account-window snapshots — captured at flow start and complete.
    // Per-phase snapshots live on PhaseState. See `WindowSnapshot`
    // for field semantics and the "fail-open" convention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_at_start: Option<WindowSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_at_complete: Option<WindowSnapshot>,
}
