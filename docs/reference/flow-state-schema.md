---
title: FLOW State Schema
nav_order: 11
parent: Reference
---

# FLOW State Schema

State files live in `.flow-states/` at the project root, named after the branch:

```text
.flow-states/app-payment-webhooks.json
.flow-states/app-payment-webhooks.log
.flow-states/app-payment-webhooks-phases.json
.flow-states/app-payment-webhooks-ci-passed
.flow-states/user-profile-redesign.json
.flow-states/user-profile-redesign.log
.flow-states/user-profile-redesign-phases.json
.flow-states/user-profile-redesign-ci-passed
```

Each feature has up to four files: the state file (`.json`), the log file (`.log`), a frozen copy of `flow-phases.json` (`-phases.json`), and a CI sentinel (`-ci-passed`). The CI sentinel caches the last passing `bin/flow ci` snapshot so subsequent runs skip automatically when nothing changed (use `--force` to bypass). The sentinel is also automatically refreshed after `finalize-commit` when `git pull` does not introduce new content, preserving the optimization across commits. Multiple features can run simultaneously with no conflicts. The directory is added to `.git/info/exclude` by `/flow-start` (per-repo, not committed). Created by `/flow-start`, deleted by `/flow-complete`.

**State files are local to each machine.** In a multi-engineer team, each engineer's `.flow-states/` directory only contains their own features. GitHub (issues, PRs, labels) is the shared coordination layer visible to all engineers. The "Flow In-Progress" label on issues is the mechanism for cross-engineer WIP detection — see `/flow-issues`.

The frozen phases file is a snapshot of `flow-phases.json` taken at start time. Scripts use it instead of the live plugin source so that phase config changes during FLOW development don't break in-progress features.

---

## Full Schema

```json
{
  "schema_version": 1,
  "branch": "app-payment-webhooks",
  "relative_cwd": "",
  "repo": "org/repo",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "started_at": "2026-02-20T10:00:00-08:00",
  "current_phase": "flow-plan",
  "prompt": "fix #83 and #89 — close issues at complete time",
  "files": {
    "plan": null,
    "dag": null,
    "log": ".flow-states/app-payment-webhooks.log",
    "state": ".flow-states/app-payment-webhooks.json"
  },
  "plan_file": null,
  "session_id": null,
  "transcript_path": null,
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-plan": {"continue": "auto", "dag": "auto"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-code-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto"
  },
  "phases": {
    "flow-start": {
      "name": "Start",
      "status": "complete",
      "started_at": "2026-02-20T10:00:00-08:00",
      "completed_at": "2026-02-20T10:05:00-08:00",
      "session_started_at": null,
      "cumulative_seconds": 300,
      "visit_count": 1
    },
    "flow-plan": {
      "name": "Plan",
      "status": "in_progress",
      "started_at": "2026-02-20T10:05:00-08:00",
      "completed_at": null,
      "session_started_at": "2026-02-20T10:30:00-08:00",
      "cumulative_seconds": 1800,
      "visit_count": 2
    },
    "flow-code": {
      "name": "Code",
      "status": "pending",
      "started_at": null,
      "completed_at": null,
      "session_started_at": null,
      "cumulative_seconds": 0,
      "visit_count": 0
    }
  },
  "phase_transitions": [],
  "issues_filed": [],
  "findings": [],
  "slack_thread_ts": null,
  "slack_notifications": []
}
```

---

## Top-Level Fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | integer | Schema version marker — currently `1` |
| `branch` | string | Git branch name — slug format. Canonical identity field. Feature name and worktree path are derived from this at read time |
| `relative_cwd` | string | Subdirectory inside the project root where the user started the flow, captured by `start-init` from `cwd.strip_prefix(project_root())`. Empty string means the flow operates at the worktree root (the common case). Non-empty values (e.g. `"api"` or `"packages/api"`) tell `start-workspace` to return a `worktree_cwd` that includes the suffix so the agent lands in the same subdirectory after the worktree is created, and tell every `bin/flow` subcommand's cwd-drift guard which directory to enforce. Defaults to empty for state files written before this field existed |
| `repo` | string / null | GitHub repo in `owner/repo` format, cached during `/flow-start`. Used by `bin/flow issue` to avoid repeated `git remote` calls. Null if detection fails |
| `pr_number` | integer / null | GitHub PR number. Null during early Start (before PR creation) when created by `init-state` — backfilled by `start-workspace` after PR creation |
| `pr_url` | string / null | Full GitHub PR URL. Null during early Start — backfilled by `start-workspace` after PR creation |
| `started_at` | ISO 8601 | When the feature was started (Phase 1 entry) |
| `current_phase` | string | The currently active phase key (e.g. `"flow-code"`) |
| `files` | object | Structured artifact file paths — see [Files Object](#files-object) |
| `plan_file` | string / null | Legacy: absolute path to the plan file. Superseded by `files.plan` — kept for backward compatibility |
| `session_id` | string / null | Claude Code session UUID — set by Stop hook from hook stdin |
| `transcript_path` | string / null | Absolute path to session transcript .jsonl — set by Stop hook from hook stdin |
| `skills` | object / absent | Per-skill autonomy settings copied from `.flow.json` by `/flow-start` — see [Skills Object](#skills-object) |
| `commit_format` | string / absent | Commit message format: `"full"` or `"title-only"`. Copied from `.flow.json` by `/flow-start`. Absent when `.flow.json` has no `commit_format` key — skills default to `"full"` |
| `start_step` | integer | Current Start phase step (0-5). Set by `init-state --start-step` at creation, then updated by `start-step` subcommand at each step boundary. Used by the TUI to show "step 3 of 5" in the Start phase annotation. Absent when Start is not in progress |
| `start_steps_total` | integer | Total number of Start phase steps (hardcoded 5). Set by `init-state --start-steps-total` at creation. Used by the TUI for "step N of M" display |
| `plan_step` | integer | Current Plan phase step (0-4). Set via `set-timestamp` (standard path) or `plan-extract` (extracted path) at each step boundary. Used by the TUI to show "step 2 of 4" in the Plan phase annotation. Absent when Plan is not in progress |
| `plan_steps_total` | integer | Total number of Plan phase steps (hardcoded 4). Set via `set-timestamp` (standard path) or `plan-extract` (extracted path) after phase entry. Used by the TUI for "step N of M" display |
| `code_review_step` | integer | Last completed Code Review step (0-4). Set to 0 on phase entry, incremented after each step (1=Gather, 2=Launch, 3=Triage, 4=Fix). Used by the TUI and for resume after context compaction |
| `code_review_steps_total` | integer | Total number of Code Review steps (hardcoded 4). Set via `set-timestamp` after phase entry. Used by the TUI for "step N of M" display |
| `code_tasks_total` | integer / absent | Total number of implementation tasks from the plan. Set by Phase 2 (Plan) Step 4 via `set-timestamp`. Used by the TUI to show "task 3 of 8" in the Code phase annotation. Absent in state files created before v0.40 |
| `code_task_name` | string / absent | Short description of the current Code task from the plan. Set by Phase 3 (Code) via `set-timestamp` before each task starts. Used by the TUI to show "Update tests - task 2 of 3" in the Code phase annotation. Absent when Code phase is not in progress or in state files created before the feature was added |
| `learn_step` | integer | Last completed Learn step (0-6). Set via `set-timestamp` at each step boundary. TUI displays `learn_step + 1` as the current step. Used for resume and TUI annotation "gathering sources - step 1 of 7" |
| `learn_steps_total` | integer | Total number of Learn phase steps (hardcoded 7). Set via `set-timestamp` after phase entry. Used by the TUI for "step N of M" display |
| `complete_step` | integer | Current Complete phase step (1-6). Set by Rust commands at each step boundary. Used by the TUI to show "merging PR - step 5 of 6" and as resume point for CI gate loops |
| `complete_steps_total` | integer | Total number of Complete phase steps (hardcoded 6). Set by `complete-fast` or `complete-preflight` after phase entry. Used by the TUI for "step N of M" display |
| `_continue_pending` | string | Child skill or action currently executing. Phase skills set this before invoking a child skill so the Stop hook (`stop-continue.py`) blocks the turn from ending and forces continuation. Values are either a child skill name (e.g. `decompose`) or the action `commit` (used by flow-code, flow-code-review, flow-learn, and flow-complete when invoking `/flow:flow-commit`). Cleared in three places: by the Stop hook after forcing continuation, by `finalize-commit` on error (to prevent blind phase advancement after a failed commit — conflict status is preserved for retry), and by `phase_enter()` on phase entry (to prevent stale flags from a previous phase). Empty string or absent means no continuation pending. |
| `_continue_context` | string | Specific next-step instructions for the model after a child skill returns. Written by phase skills before `_continue_pending`, read and cleared by the Stop hook. Also cleared by `finalize-commit` on error and by `phase_enter()` on phase entry (same lifecycle as `_continue_pending`). Included in the block reason so the model knows what to do after the turn boundary. Empty string or absent means use the generic fallback message. |
| `_blocked` | ISO 8601 / null | Timestamp when the flow was blocked on AskUserQuestion. Set by PreToolUse hook (`bin/flow hook validate-ask-user`) when allowing a prompt through. Cleared by PostToolUse hook (`bin/flow clear-blocked`) after user responds and by Stop hook (`bin/flow hook stop-continue`) as a safety net for crashed sessions. Transient. |
| `_last_failure` | object / null | API error context from the last StopFailure event. Contains `type` (string — error category, e.g. `rate_limit`, `auth_failure`, `network_timeout`), `message` (string — error details), and `timestamp` (ISO 8601 — when the failure occurred). Written by StopFailure hook (`bin/flow hook stop-failure`). Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `_auto_continue` | string | Command to invoke next (e.g. `/flow:flow-plan`). Set by `phase_complete()` when `skills.<phase>.continue` is `"auto"`. Cleared by `phase_enter()` when the next phase starts. A PreToolUse hook on AskUserQuestion automatically answers prompts via `updatedInput` while this flag is set. |
| `_stop_instructed` | boolean | Discussion-mode flag. Set to `true` by the Stop hook (`check_first_stop`) on the first stop event during an active flow — either a pure discussion-mode block (no pending) or a conditional continuation (pending set, with user-message awareness). On subsequent stops the flag is already `true` and `check_first_stop` allows stop through to `check_continue`. Cleared by `check_continue` when `_continue_pending` is consumed (atomically in the same `mutate_state` closure) and by `phase_enter()` on phase entry. Not cleared on session mismatch — `phase_enter()` handles the reset when the new session enters a phase. Transient. |
| `prompt` | string | The full text passed to `/flow-start` — used by Plan as feature description and by Complete to extract `#N` issue references for auto-closing |
| `notes` | array | Corrections captured via `/flow-note` — see [Notes Array](#notes-array) |
| `phase_transitions` | array | Phase entry log recording every `phase_enter()` call with from/to/timestamp and optional reason — see [Phase Transitions Array](#phase-transitions-array) |
| `issues_filed` | array | GitHub issues filed during the feature — see [Issues Filed Array](#issues-filed-array) |
| `findings` | array | Triage findings from Code Review and Learn phases — see [Findings Array](#findings-array) |
| `compact_summary` | string / null | Conversation summary from last compaction. Written by PostCompact hook. Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `compact_cwd` | string / null | CWD at last compaction time. Written by PostCompact hook. Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `compact_count` | integer | Total number of context compactions during this feature. Incremented by PostCompact hook. Permanent. |
| `slack_thread_ts` | string / null | Slack message timestamp of the initial thread message. Set by Start phase after first `notify-slack` call. Used by subsequent phases as `thread_ts` to reply in the same thread. Null or absent if Slack is not configured. |
| `slack_notifications` | array | Slack notifications sent during the feature — see [Slack Notifications Array](#slack-notifications-array) |

---

## Phase Fields

Each phase entry has identical fields regardless of status.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Human-readable phase name |
| `status` | string | `pending`, `in_progress`, or `complete` |
| `started_at` | ISO 8601 / null | First time this phase was entered — **never overwritten** |
| `completed_at` | ISO 8601 / null | Most recent time this phase was exited — updated on every completion |
| `session_started_at` | ISO 8601 / null | Timestamp when current session entered this phase — reset to `now()` on resume, cleared to `null` on clean exit |
| `cumulative_seconds` | integer | Total seconds spent in this phase across all visits — additive |
| `visit_count` | integer | Number of times this phase has been entered |

---

## Timing Rules

- `started_at` is set on first entry and **never changed again**
- `completed_at` is set on every exit — reflects the most recent completion
- `session_started_at` is set on entry and cleared to `null` on exit
- `cumulative_seconds` increments by `(exit_time - session_started_at)` on each clean exit

---

## Skills Object

Copied from `.flow.json` into the state file by `/flow-start`. Phase skills read autonomy config from the state file rather than `.flow.json`, because `.flow.json` lives at the project root and is not accessible from worktrees.

Present only when `.flow.json` contains a `skills` key (i.e., after running `/flow-prime` with Customize or a preset). Phase skills that don't find a `skills` key in the state file fall back to built-in defaults.

Each value is either a **bare string** (`"auto"` or `"manual"`) or an **object** with per-axis settings (e.g. `{"continue": "auto", "commit": "manual"}`). Skills with a single autonomy axis — `flow-abort`, `flow-complete` — typically use the bare form. Phase skills that expose both a commit axis (per-task commit approval) and a continue axis (phase transition approval) — `flow-code`, `flow-code-review`, `flow-learn` — use the object form. The two shapes are represented in Rust as `SkillConfig::Simple(String)` and `SkillConfig::Detailed(IndexMap<String, String>)` in `src/state.rs`; consumers that read the config via `serde_json::Value` (like the `validate-ask-user` PreToolUse hook) accept both shapes by checking `Value::as_str() == Some("auto")` OR `Value::get("continue").and_then(|c| c.as_str()) == Some("auto")`.

```json
"skills": {
  "flow-start": {"continue": "manual"},
  "flow-code": {"commit": "manual", "continue": "manual"},
  "flow-code-review": {"commit": "auto", "continue": "auto"},
  "flow-learn": {"commit": "auto", "continue": "auto"},
  "flow-abort": "auto",
  "flow-complete": "auto"
}
```

---

## Files Object

Structured artifact file paths using relative paths (relative to project root)
for portability. Created by `/flow-start` with `plan` and `dag` set to `null`.
Updated by `/flow-plan` via `set-timestamp --set files.plan=<path>` and
`set-timestamp --set files.dag=<path>`.

```json
"files": {
  "plan": ".flow-states/app-payment-webhooks-plan.md",
  "dag": ".flow-states/app-payment-webhooks-dag.md",
  "log": ".flow-states/app-payment-webhooks.log",
  "state": ".flow-states/app-payment-webhooks.json"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `plan` | string / null | Relative path to the implementation plan file — set by Phase 2 |
| `dag` | string / null | Relative path to the DAG analysis file — set by Phase 2 |
| `log` | string | Relative path to the session log file — set at creation |
| `state` | string | Relative path to this state file — set at creation |

---

## Notes Array

Populated throughout the session by `/flow-note`. Survives compaction
and session restarts. Read by Learn as a primary source.

```json
"notes": [
  {
    "phase": "flow-code",
    "phase_name": "Code",
    "timestamp": "2026-02-20T14:23:00-08:00",
    "type": "correction",
    "note": "Never assume branch-behind is unlikely — multiple active sessions means branches regularly fall behind main"
  }
]
```

---

## Phase Transitions Array

Populated by `phase_enter()` on every phase entry. Records the journey
through phases, enabling the Learn phase to identify rework patterns.

```json
"phase_transitions": [
  {"from": "flow-start", "to": "flow-plan", "timestamp": "2026-02-20T10:05:00-08:00"},
  {"from": "flow-plan", "to": "flow-code", "timestamp": "2026-02-20T10:30:00-08:00"},
  {"from": "flow-code", "to": "flow-code-review", "timestamp": "2026-02-20T14:00:00-08:00"},
  {"from": "flow-code-review", "to": "flow-code", "timestamp": "2026-02-20T14:30:00-08:00", "reason": "test failures"}
]
```

| Field | Type | Description |
|-------|------|-------------|
| `from` | string / null | Phase key before transition. Null on first entry |
| `to` | string | Phase key being entered |
| `timestamp` | ISO 8601 | When the transition occurred |
| `reason` | string / absent | Optional reason for backward transitions |

---

## Issues Filed Array

Populated by `bin/flow add-issue` whenever a skill files a GitHub issue
via `bin/flow issue`. Surfaced in the Complete phase PR body and Done banner.

```json
"issues_filed": [
  {
    "label": "Rule",
    "title": "Add rule: never use git checkout for file ops",
    "url": "https://github.com/org/repo/issues/42",
    "phase": "flow-learn",
    "phase_name": "Learn",
    "timestamp": "2026-03-12T10:00:00-07:00"
  }
]
```

| Field | Type | Description |
|-------|------|-------------|
| `label` | string | Issue category: Rule, Flow, Flaky Test, Tech Debt, or Documentation Drift |
| `title` | string | Issue title as filed on GitHub |
| `url` | string | Full GitHub issue URL |
| `phase` | string | Phase key where the issue was filed (e.g. `"flow-learn"`) |
| `phase_name` | string | Human-readable phase name |
| `timestamp` | ISO 8601 | When the issue was filed |

---

## Findings Array

Populated by `bin/flow add-finding` during Code Review (Phase 4) and Learn
(Phase 5) triage. Each entry records a finding, its triage outcome, and the
reasoning. Rendered in the Complete phase Done banner as "Code Review Findings"
and "Learn Findings" sections.

```json
"findings": [
  {
    "finding": "Unused import in parser.rs",
    "reason": "False positive — import used in macro expansion",
    "outcome": "dismissed",
    "phase": "flow-code-review",
    "phase_name": "Code Review",
    "timestamp": "2026-03-12T10:30:00-07:00"
  },
  {
    "finding": "No rule for error handling pattern",
    "reason": "Gap identified during learn analysis",
    "outcome": "rule_written",
    "phase": "flow-learn",
    "phase_name": "Learn",
    "path": ".claude/rules/error-handling.md",
    "timestamp": "2026-03-12T10:45:00-07:00"
  }
]
```

| Field | Type | Description |
|-------|------|-------------|
| `finding` | string | Description of what was found |
| `reason` | string | Why this outcome was chosen |
| `outcome` | string | Triage outcome: `fixed`, `dismissed`, `filed`, `rule_written`, or `rule_clarified` |
| `phase` | string | Phase key where the finding was triaged (e.g. `"flow-code-review"`, `"flow-learn"`) |
| `phase_name` | string | Human-readable phase name |
| `timestamp` | ISO 8601 | When the finding was recorded |
| `issue_url` | string (optional) | GitHub issue URL — present when outcome is `filed` |
| `path` | string (optional) | Rule file path — present when outcome is `rule_written` or `rule_clarified` |

---

## Slack Notifications Array

Populated by `bin/flow add-notification` when a phase skill sends a Slack message via `bin/flow notify-slack`. Only present when Slack is configured via `/flow:flow-prime`. Surfaced in the Complete phase PR body.

```json
"slack_notifications": [
  {
    "phase": "flow-start",
    "phase_name": "Start",
    "ts": "1234567890.123456",
    "thread_ts": "1234567890.123456",
    "message_preview": "Feature started",
    "timestamp": "2026-03-20T10:00:00-07:00"
  }
]
```

| Field | Type | Description |
|-------|------|-------------|
| `phase` | string | Phase key that sent the notification (e.g. `"flow-start"`) |
| `phase_name` | string | Human-readable phase name |
| `ts` | string | Slack message timestamp (unique ID for the posted message) |
| `thread_ts` | string | Slack thread timestamp (matches `slack_thread_ts` for thread replies) |
| `message_preview` | string | First 100 characters of the message text |
| `timestamp` | ISO 8601 | When the notification was sent |

---

## Plan File

The plan lives at `.flow-states/<branch>-plan.md` alongside other feature artifacts. The state file stores the relative path in `files.plan`. The plan file includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## State Machine

Valid phase transitions are defined in `flow-phases.json` at the plugin root. Forward progression is always valid. Backward transitions are limited per phase.

Valid transitions are defined in `flow-phases.json`: Code can return to Plan; Code Review can return to Code or Plan.
