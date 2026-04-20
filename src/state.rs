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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseState {
    pub name: String,
    pub status: PhaseStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub session_started_at: Option<String>,
    pub cumulative_seconds: i64,
    pub visit_count: i64,
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
}
