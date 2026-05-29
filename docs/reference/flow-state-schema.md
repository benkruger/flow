---
title: FLOW State Schema
nav_order: 11
parent: Reference
---

# FLOW State Schema

State files live under `.flow-states/<branch>/` at the project root, one subdirectory per active feature:

```text
.flow-states/app-payment-webhooks/state.json
.flow-states/app-payment-webhooks/log
.flow-states/app-payment-webhooks/phases.json
.flow-states/app-payment-webhooks/ci-passed
.flow-states/user-profile-redesign/state.json
.flow-states/user-profile-redesign/log
.flow-states/user-profile-redesign/phases.json
.flow-states/user-profile-redesign/ci-passed
```

Each feature has up to four files inside its subdirectory: the state file (`state.json`), the log file (`log`), a frozen copy of `flow-phases.json` (`phases.json`), and a CI sentinel (`ci-passed`). A feature may also hold a `shared-config-approvals/` subdirectory of single-use approval markers — see [Shared-Config Approval Markers](#shared-config-approval-markers) — and a single-use `merge-approval` marker file written when the user confirms a manual-mode squash-merge — see [Merge-Approval Marker](#merge-approval-marker). The CI sentinel caches the last passing `bin/flow ci` snapshot so subsequent runs skip automatically when nothing changed (use `--force` to bypass). The sentinel is also automatically refreshed after `finalize-commit` when `git pull` does not introduce new content, preserving the optimization across commits. Multiple features can run simultaneously with no conflicts. The `.flow-states/` directory is added to `.git/info/exclude` by `/flow-start` (per-repo, not committed). Created by `/flow-start`, deleted by `/flow-complete` in a single `remove_dir_all(branch_dir)` call.

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
  "current_phase": "flow-code",
  "prompt": "fix #83 and #89 — close issues at complete time",
  "files": {
    "plan": null,
    "dag": null,
    "log": ".flow-states/app-payment-webhooks/log",
    "state": ".flow-states/app-payment-webhooks/state.json"
  },
  "plan_file": null,
  "session_id": null,
  "transcript_path": null,
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": {"continue": "auto"},
    "flow-complete": {"continue": "auto"}
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
    "flow-code": {
      "name": "Code",
      "status": "in_progress",
      "started_at": "2026-02-20T10:05:00-08:00",
      "completed_at": null,
      "session_started_at": "2026-02-20T10:30:00-08:00",
      "cumulative_seconds": 1800,
      "visit_count": 2
    },
    "flow-review": {
      "name": "Review",
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
| `relative_cwd` | string | Subdirectory inside the project root where the user started the flow, captured by `start-init` from `cwd.strip_prefix(project_root())`. Empty string means the flow operates at the worktree root (the common case). Non-empty values (e.g. `"api"` or `"packages/api"`) tell `start-workspace` to include the suffix in its **absolute** `worktree_cwd` return value so the agent lands in the same subdirectory after the worktree is created, and tell every `bin/flow` subcommand's cwd-drift guard which directory to enforce. The skill's `cd <worktree_cwd>` works from any bash cwd because `worktree_cwd` is absolute; this matters when the user launches Claude at the repo root and `cd <app>` before invoking `/flow:flow-start`. Defaults to empty for state files written before this field existed |
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
| `review_step` | integer | Last completed Review step (0-4). Set to 0 on phase entry, incremented after each step (1=Gather, 2=Launch, 3=Triage, 4=Fix). Used by the TUI and for resume after context compaction |
| `review_steps_total` | integer | Total number of Review steps (hardcoded 4). Set via `set-timestamp` after phase entry. Used by the TUI for "step N of M" display |
| `code_tasks_total` | integer / absent | Total number of implementation tasks from the plan. Set by Phase 1 (Start) Step 5 via `set-timestamp`, derived from a count of `#### Task N:` headings in the extracted `plan.md` (returned by `bin/flow plan-from-issue` in its success envelope as `tasks_total`). Used by the TUI to show "task 3 of 8" in the Code phase annotation. Absent in state files created before v0.40 |
| `code_task_name` | string / absent | Short description of the current Code task from the plan. Set by Phase 2 (Code) via `set-timestamp` before each task starts. Used by the TUI to show "Update tests - task 2 of 3" in the Code phase annotation. Absent when Code phase is not in progress or in state files created before the feature was added |
| `learn_step` | integer | Last completed Learn step (0-6). Set via `set-timestamp` at each step boundary. TUI displays `learn_step + 1` as the current step. Used for resume and TUI annotation "gathering sources - step 1 of 7" |
| `learn_steps_total` | integer | Total number of Learn phase steps (hardcoded 7). Set via `set-timestamp` after phase entry. Used by the TUI for "step N of M" display |
| `complete_step` | integer | Current Complete phase step (1-5). Set by Rust commands at each step boundary. Used by the TUI to show "merging PR - step 4 of 5" and as resume point for CI gate loops |
| `complete_steps_total` | integer | Total number of Complete phase steps (hardcoded 5). Set by `complete-fast` or `complete-preflight` after phase entry. Used by the TUI for "step N of M" display |
| `_continue_pending` | string | Child skill or action currently executing. Phase skills set this before invoking a child skill so the Stop hook (`bin/flow hook stop-continue`) blocks the turn from ending and forces continuation. Values are either a child skill name (e.g. `decompose`) or the action `commit` (used by flow-code, flow-review, flow-learn, and flow-complete when invoking `/flow:flow-commit`). Cleared in three places: by the Stop hook after forcing continuation, by `finalize-commit` on error (to prevent blind phase advancement after a failed commit — conflict status is preserved for retry), and by `phase_enter()` on phase entry (to prevent stale flags from a previous phase). Empty string or absent means no continuation pending. |
| `_continue_context` | string | Specific next-step instructions for the model after a child skill returns. Written by phase skills before `_continue_pending`, read and cleared by the Stop hook. Also cleared by `finalize-commit` on error and by `phase_enter()` on phase entry (same lifecycle as `_continue_pending`). Included in the block reason so the model knows what to do after the turn boundary. Empty string or absent means use the generic fallback message. |
| `_blocked` | ISO 8601 / null | Timestamp when the flow was blocked on AskUserQuestion. Set by PreToolUse hook (`bin/flow hook validate-ask-user`) when allowing a prompt through. Cleared by PostToolUse hook (`bin/flow clear-blocked`) after user responds and by Stop hook (`bin/flow hook stop-continue`) as a safety net for crashed sessions. Transient. |
| `_last_failure` | object / null | API error context from the last StopFailure event. Contains `type` (string — error category, e.g. `rate_limit`, `auth_failure`, `network_timeout`), `message` (string — error details), and `timestamp` (ISO 8601 — when the failure occurred). Written by StopFailure hook (`bin/flow hook stop-failure`). Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `_auto_continue` | string | Command to invoke next (e.g. `/flow:flow-code`). Set by `phase_complete()` when `skills.<phase>.continue` is `"auto"`. Cleared by `phase_enter()` when the next phase starts. A PreToolUse hook on AskUserQuestion automatically answers prompts via `updatedInput` while this flag is set. |
| `_halt_pending` | boolean | Halt-pause flag for the user-initiated pause contract in autonomous flows. Set to `true` by the Stop hook's `check_autonomous_stop` predicate when the user has typed a prose message after the model's most recent Skill action AND the current phase is in-progress + `auto`. Cleared by `bin/flow clear-halt` (invoked exclusively by `/flow:flow-continue` — Layer 1 of the user-only-skill enforcement chain blocks any other invoker). Also cleared by `check_autonomous_stop` when the current phase is no longer in-progress OR no longer configured `auto` (stale halt residue from a prior phase). Default-false on missing or wrong-type values. Persists across multiple Stop events — every subsequent Stop continues to block (Rule 2) until the user invokes `/flow:flow-continue`. `_continue_pending` is preserved across every set and clear so the multi-child-skill resume path can read it once the halt is cleared. See `.claude/rules/autonomous-phase-discipline.md`. Transient. |
| `_last_observed_code_task` | integer | Hook-managed counter-tracking field for the autonomous-mode stalling-pattern refusal-text swap. Set by Stop hook's `check_autonomous_stop` when the current phase is in-progress autonomous flow-code AND no halt is pending AND no user message has appeared since the last Skill action. Records the `code_task` value at the prior Stop event. Compared on the next Stop to detect whether the model advanced the plan task. **First-observation semantic**: when the field is absent (initial Stop after a fresh flow-code entry), the hook initializes it to the current `code_task` and sets the paired count to 0 — the pointed-text swap does NOT fire on the first Stop of a flow-code window even when count would otherwise meet the threshold. Cleared by `phase_enter()` on every phase entry (alongside `_halt_pending`). Default 0 on missing or wrong-type. Model writes rejected via `MODEL_DENIED_FIELDS` in `src/commands/set_timestamp.rs` (CLI path only — direct Edit/Write tool calls against the state file remain a broader trust surface, the same boundary as `_halt_pending`). See `.claude/rules/autonomous-phase-discipline.md` "Forbidden Stalling Frames". Transient. |
| `_consecutive_unchanged_count` | integer | Hook-managed counter paired with `_last_observed_code_task`. Increments by 1 on each Stop in autonomous flow-code where `code_task` is unchanged; resets to 0 when `code_task` advances OR when `_last_observed_code_task` is initialized for the first time. When the count reaches `CONSECUTIVE_UNCHANGED_THRESHOLD` (currently 3), the Stop hook's Rule 1 refusal swaps the encouraging `RULE_1_STOP_REFUSED_MESSAGE` for `RULE_1_STOP_REFUSED_POINTED_MESSAGE`, which names the stalling pattern explicitly. Cleared by `phase_enter()` on phase entry. Default 0 on missing or wrong-type. Model writes rejected via `MODEL_DENIED_FIELDS` (CLI path only — direct Edit/Write tool calls against the state file remain a broader trust surface). Transient. |
| `prompt` | string | The full text passed to `/flow-start` — used by Plan as feature description and by Complete to extract `#N` issue references for auto-closing |
| `notes` | array | Corrections captured via `/flow-note` — see [Notes Array](#notes-array) |
| `phase_transitions` | array | Phase entry log recording every `phase_enter()` call with from/to/timestamp and optional reason — see [Phase Transitions Array](#phase-transitions-array) |
| `issues_filed` | array | GitHub issues filed during the feature — see [Issues Filed Array](#issues-filed-array) |
| `findings` | array | Triage findings from Review and Learn phases — see [Findings Array](#findings-array) |
| `compact_summary` | string / null | Conversation summary from last compaction. Written by PostCompact hook. Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `compact_cwd` | string / null | CWD at last compaction time. Written by PostCompact hook. Currently has no consumer (session-start consumer removed in PR #938). Transient. |
| `compact_count` | integer | Total number of context compactions during this feature. Incremented by PostCompact hook. Permanent. |
| `slack_thread_ts` | string / null | Slack message timestamp of the initial thread message. Set by Start phase after first `notify-slack` call. Used by subsequent phases as `thread_ts` to reply in the same thread. Null or absent if Slack is not configured. |
| `slack_notifications` | array | Slack notifications sent during the feature — see [Slack Notifications Array](#slack-notifications-array) |
| `window_at_start` | object / absent | Account-window snapshot captured at flow-start. See [Window Snapshot](#window-snapshot). Absent when not yet populated or when capture failed. |
| `window_at_complete` | object / absent | Account-window snapshot captured at Phase 5 finalize. See [Window Snapshot](#window-snapshot). Absent until Complete runs. |

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
| `window_at_enter` | object / absent | Account-window snapshot captured on phase entry. See [Window Snapshot](#window-snapshot). Absent until phase entry runs or when capture failed. |
| `window_at_complete` | object / absent | Account-window snapshot captured on phase finalize. See [Window Snapshot](#window-snapshot). Absent until phase finalize runs. |
| `step_snapshots` | array | Array of [Step Snapshots](#step-snapshot) appended on each step-counter increment (`code_task`, `review_step`, `learn_step`, `complete_step`). Empty until the phase begins incrementing its step counter. Bounded by step count per phase (typically <30 entries; up to ~10 KB for a long Code phase). |
| `agents_skipped` | array / absent | Review-only and Learn-only. Entries of `{agent: string, reason: string, timestamp: ISO 8601}` appended by `bin/flow add-skipped-agent` when the calling skill's failure classification detects an agent returned an external-failure marker OR exhausted the retry cap. `reason` is a positive allowlist: `rate_limit`, `api_error`, `other`, or `exhausted_retries`. Absent on every phase except `flow-review` / `flow-learn`, and absent on runs that complete with no skipped agents. Consumed by `phase-finalize`'s `agents_skipped` gate AND the required-agents gate — see below. |
| `agents_returned` | array / absent | Review-only and Learn-only. Entries of `{agent: string, timestamp: ISO 8601}` appended by `bin/flow record-agent-return` after the calling skill's Class 3 (Normal completion) classification. The recording subcommand reads the persisted Claude Code transcript and confirms an Agent tool_use/tool_result pair exists for `subagent_type: "flow:<agent>"` after the most recent `phase-enter --phase <phase>` Bash marker — closing the inline-synthesis bypass where a model could write findings without actually invoking the agent. Consumed by `phase-finalize`'s required-agents gate — see below. |
| `agent_retry_counts` | object / absent | Review-only and Learn-only. Map of `<agent_name> -> count` (`0..=3`). The calling skill increments this counter via `bin/flow set-timestamp` when an agent's tool_result triggers Class 1 (truncation), Class 2 (external failure), or recording failure. When the counter reaches 3, the skill records the agent in `agents_skipped` with reason `exhausted_retries`. Absent on phases that don't invoke sub-agents. |

### Agents-Skipped Gate

`bin/flow phase-finalize --phase flow-review` gates on the
`agents_skipped` array. When the array is non-empty AND
`--accept-skipped-agents` is absent, the command returns:

```json
{
  "status": "error",
  "reason": "agents_skipped",
  "skipped": [{"agent": "reviewer", "reason": "rate_limit", "timestamp": "..."}],
  "message": "<count> agents skipped during flow-review; pass --accept-skipped-agents to proceed"
}
```

The gate runs after the state-file existence check and before
any state mutation, so a rejection leaves the phase in
`in_progress` for caller retry. Pass `--accept-skipped-agents`
to bypass the gate; the skipped entries remain in state for the
Learn-phase audit.

### Required-Agents Gate

`bin/flow phase-finalize` also gates on the union of
`agents_returned` and `agents_skipped` against the per-phase
`REQUIRED_AGENTS` constant (`src/required_agents.rs`):

- `flow-review` requires `{reviewer, pre-mortem, adversarial, documentation}`
- `flow-learn` requires `{learn-analyst}`
- All other phases have no required agents and bypass this gate

The gate computes `missing = required \ (returned ∪ skipped)` and
refuses the phase when `missing` is non-empty:

```json
{
  "status": "error",
  "reason": "required_agent_not_returned",
  "missing": ["adversarial", "documentation"],
  "message": "<count> required agents for flow-review neither returned nor skipped: [...]"
}
```

Fail-closed on wrong-type `agents_returned`: when the field is
present but not an array (string, number, object), the gate
refuses the phase with the same `required_agent_not_returned`
reason. This composes with the `agents_skipped` gate: when
`--accept-skipped-agents` is set, the skipped-non-empty gate is
bypassed but the required-agents gate still requires every
required agent to appear in EITHER `agents_returned` OR
`agents_skipped`. Malformed entries (missing the `agent` field)
are skipped during the composition; only entries with a valid
`agent` string contribute to the accounted-for set.

### Required-Agents Gate v1 Boundaries

The required-agents gate is the load-bearing protection against
"model fabricates findings without invoking the agent." Two v1
limitations are documented here so consumers know which surfaces
the gate covers and which it does not:

**Skip reasons are procedural, not transcript-verified.**
`bin/flow add-skipped-agent` writes the entry unconditionally
when the reason normalizes to one of `rate_limit`, `api_error`,
`other`, `exhausted_retries`. A model that calls
`add-skipped-agent --reason exhausted_retries` without actually
exhausting three retries satisfies the `agents_skipped` half of
the gate the same way a legitimate `api_error` entry does.
Exhausted and skipped agents surface to the user in the Complete
Done banner's Skipped Agents section, sourced from
`phases.<phase>.agents_skipped`. Procedural enforcement of the
retry cap lives in the calling SKILL.md's retry-3-then-skip loop.
A future PR may mechanically gate `exhausted_retries` against
`phases.<phase>.agent_retry_counts.<agent> >= 3`; that
tightening is out of scope for v1.

**Cross-session transcript contamination.**
`bin/flow record-agent-return` reads `session_id` and
`transcript_path` from the per-branch state file and walks the
JSONL at that path for the LAST `bin/flow phase-enter --phase
<phase>` marker. Two flows running in the SAME Claude Code
session (the user opened a second worktree in the same session
and ran `flow-start` on a different branch) share the same
transcript path. Each branch's state file records the same
`session_id`. When both branches enter `flow-review` in the same
session, the verifier's "LAST phase-enter --phase flow-review"
scan may anchor on the OTHER branch's marker, accepting or
rejecting an agent invocation against the wrong scan window.
The typical usage pattern — one Claude Code session per
worktree (distinct terminal windows) — produces distinct
`session_id`s, isolating the transcripts. A future PR may
extend the phase-enter marker to include the branch name so the
verifier's scan is branch-scoped; that tightening is out of
scope for v1.

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

Each value is an **object** with per-axis settings. `/flow-prime` normalizes every entry to the object shape before writing `.flow.json`, so `/flow-start` copies object-shaped entries into the state file for all six skills. The committing phase skills — `flow-code`, `flow-review`, `flow-learn` — carry both a commit axis (per-task commit approval) and a continue axis (phase transition approval): `{"commit": ..., "continue": ...}`. The single-axis skills — `flow-start`, `flow-abort`, `flow-complete` — carry only the continue axis: `{"continue": ...}`. The shared `resolve-skill-mode` subcommand reads `commit` and `continue` from the object and is the single resolution path for all six skills.

The object shape is represented in Rust as `SkillConfig::Detailed(IndexMap<String, String>)` in `src/state.rs`; `SkillConfig::Simple(String)` remains as a tolerated parse for a hand-edited bare-string entry, and the `validate-ask-user` PreToolUse hook accepts either shape by checking `Value::as_str() == Some("auto")` OR `Value::get("continue").and_then(|c| c.as_str()) == Some("auto")`.

```json
"skills": {
  "flow-start": {"continue": "manual"},
  "flow-code": {"commit": "manual", "continue": "manual"},
  "flow-review": {"commit": "auto", "continue": "auto"},
  "flow-learn": {"commit": "auto", "continue": "auto"},
  "flow-abort": {"continue": "auto"},
  "flow-complete": {"continue": "auto"}
}
```

---

## Files Object

Structured artifact file paths using relative paths (relative to project root)
for portability. Created by `/flow-start` with `plan` and `dag` set to `null`.
`files.plan` is populated at Phase 1 (Start) Step 5 by `bin/flow plan-from-issue`,
which extracts the plan from the issue body's
`<!-- FLOW-PLAN-BEGIN -->`/`<!-- FLOW-PLAN-END -->` sentinels and writes it to
`.flow-states/<branch>/plan.md`. `files.dag` is written by the pre-decompose
flow (the `/flow-plan` utility skill) when a DAG analysis is produced.

```json
"files": {
  "plan": ".flow-states/app-payment-webhooks/plan.md",
  "dag": ".flow-states/app-payment-webhooks/dag.md",
  "log": ".flow-states/app-payment-webhooks/log",
  "state": ".flow-states/app-payment-webhooks/state.json"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `plan` | string / null | Relative path to the implementation plan file — set by Phase 1 Step 5 (`bin/flow plan-from-issue`) |
| `dag` | string / null | Relative path to the DAG analysis file — written by the pre-decompose flow when applicable |
| `log` | string | Relative path to the session log file — set at creation |
| `state` | string | Relative path to this state file — set at creation |

These entries are **descriptive** — they record where the artifacts live so consumers (e.g., the Plan-phase resume check, `format-status`) can read them. They are NOT the source of truth for write destinations. The canonical destination for every managed FLOW artifact is computed by `FlowPaths` (`src/flow_paths.rs`) as a pure function of `(project_root, branch)`. `bin/flow write-rule` enforces this at the CLI layer: when `--path` names a managed artifact (`plan.md`, `dag.md`, `commit-msg.txt`, `.flow-issue-body`, `orchestrate-queue.json`), any value that doesn't lexically normalize to the `FlowPaths`-computed destination is rejected with `step: "path_canonicalization"`. The commit-message file (`<branch_dir>/commit-msg.txt`, via `FlowPaths::commit_msg`) is intentionally absent from `files.*` for this reason — its path is recoverable from `(project_root, branch)` alone, so persisting it in state would only invite drift. See `.claude/rules/file-tool-preflights.md` "Managed-Artifact Canonicalization Gate (CLI Layer)".

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
  {"from": "flow-start", "to": "flow-code", "timestamp": "2026-02-20T10:30:00-08:00"},
  {"from": "flow-code", "to": "flow-review", "timestamp": "2026-02-20T14:00:00-08:00"},
  {"from": "flow-review", "to": "flow-code", "timestamp": "2026-02-20T14:30:00-08:00", "reason": "test failures"}
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
| `label` | string | Issue category: Rule, Tech Debt, or Documentation Drift |
| `title` | string | Issue title as filed on GitHub |
| `url` | string | Full GitHub issue URL |
| `phase` | string | Phase key where the issue was filed (e.g. `"flow-learn"`) |
| `phase_name` | string | Human-readable phase name |
| `timestamp` | ISO 8601 | When the issue was filed |

---

## Findings Array

Populated by `bin/flow add-finding` during Review (Phase 4) and Learn
(Phase 5) triage. Each entry records a finding, its triage outcome, and the
reasoning. Rendered in the Complete phase Done banner as "Review Findings"
and "Learn Findings" sections.

```json
"findings": [
  {
    "finding": "Unused import in parser.rs",
    "reason": "False positive — import used in macro expansion",
    "outcome": "dismissed",
    "phase": "flow-review",
    "phase_name": "Review",
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
| `phase` | string | Phase key where the finding was triaged (e.g. `"flow-review"`, `"flow-learn"`) |
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

The plan lives at `.flow-states/<branch>/plan.md` alongside other feature artifacts. The state file stores the relative path in `files.plan`. The plan file includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## Window Snapshot

Captured at every state-mutating transition (flow start, phase enter, phase finalize, step-counter increments, flow complete). Stores account-wide observations attributed to the active flow by delta — exact when running a single flow end-to-end on a quiet account, approximate otherwise.

Stored raw — never as deltas. Readers (Complete summary, `format-status`, TUI) compute deltas at read time and detect window resets (`five_hour_pct` going down between snapshots) by inspecting raw values. No `window_reset_observed` flag is stored.

Every numeric field is optional so a missing or unreadable input source (rate-limits file, transcript JSONL, cost file) leaves the field as `null` rather than failing the capture or transition.

```json
{
  "captured_at": "2026-05-04T10:00:00-07:00",
  "session_id": "abc-123",
  "model": "claude-opus-4-7",
  "five_hour_pct": 42,
  "seven_day_pct": 7,
  "session_input_tokens": 12345,
  "session_output_tokens": 67890,
  "session_cache_creation_tokens": 100,
  "session_cache_read_tokens": 9876,
  "by_model": {
    "claude-opus-4-7": {"input": 12345, "output": 67890, "cache_create": 100, "cache_read": 9876}
  },
  "turn_count": 15,
  "tool_call_count": 73,
  "context_at_last_turn_tokens": 123456,
  "context_window_pct": 61.5
}
```

| Field | Type | Description |
|-------|------|-------------|
| `captured_at` | ISO 8601 | Wall-clock time the snapshot was taken (Pacific Time, per `src/utils.rs::now`) |
| `session_id` | string / null | Claude Code session UUID at capture time, copied from `state.session_id` |
| `model` | string / null | Model in use at capture time, when known — typically derived from the most recent `assistant` message in the transcript |
| `five_hour_pct` | integer / null | 5-hour rolling rate-limit utilization, read from `~/.claude/rate-limits.json` |
| `seven_day_pct` | integer / null | 7-day rolling rate-limit utilization, read from `~/.claude/rate-limits.json` |
| `session_input_tokens` | integer / null | Sum of `message.usage.input_tokens` across every `assistant` message in the transcript |
| `session_output_tokens` | integer / null | Sum of `message.usage.output_tokens` |
| `session_cache_creation_tokens` | integer / null | Sum of `message.usage.cache_creation_input_tokens` |
| `session_cache_read_tokens` | integer / null | Sum of `message.usage.cache_read_input_tokens` |
| `by_model` | object | Per-model token totals — see [Model Tokens](#model-tokens). Empty when the transcript could not be read. Per-phase cost is token-derived downstream: `window_deltas::pair_delta` prices the per-phase `by_model` delta through `pricing::cost_for` |
| `turn_count` | integer / null | Number of `assistant` messages observed in the transcript |
| `tool_call_count` | integer / null | Number of `tool_use` blocks observed across all `assistant` messages |
| `context_at_last_turn_tokens` | integer / null | Total context window utilization (input + cache_read + cache_create + output) at the most recent assistant turn |
| `context_window_pct` | number / null | `context_at_last_turn_tokens` as a percentage of the model's context window, when known |

### Model Tokens

A single entry inside the `by_model` object — present only when at least one `assistant` message named the model. Counters are non-optional within an entry because the entry exists by construction (zero is a meaningful value once the entry is populated).

```json
"claude-opus-4-7": {
  "input": 12345,
  "output": 67890,
  "cache_create": 100,
  "cache_read": 9876
}
```

| Field | Type | Description |
|-------|------|-------------|
| `input` | integer | Sum of `message.usage.input_tokens` for this model |
| `output` | integer | Sum of `message.usage.output_tokens` for this model |
| `cache_create` | integer | Sum of `message.usage.cache_creation_input_tokens` for this model |
| `cache_read` | integer | Sum of `message.usage.cache_read_input_tokens` for this model |

---

## Step Snapshot

Appended to a phase's `step_snapshots[]` on every step-counter increment that names one of the four recognized counters: `code_task`, `review_step`, `learn_step`, `complete_step`. Each entry combines the counter value at the time of capture, the field name, and a flattened [Window Snapshot](#window-snapshot) — so each record is one flat JSON object rather than a nested `{snapshot: {...}}` shape.

```json
{
  "step": 3,
  "field": "code_task",
  "captured_at": "2026-05-04T10:30:00-07:00",
  "session_id": "abc-123",
  "five_hour_pct": 45,
  "session_input_tokens": 23456,
  "session_output_tokens": 9876,
  "by_model": {"claude-opus-4-7": {"input": 23456, "output": 9876, "cache_create": 0, "cache_read": 0}},
  "turn_count": 22,
  "tool_call_count": 110,
  "context_at_last_turn_tokens": 145000,
  "context_window_pct": 72.5
}
```

| Field | Type | Description |
|-------|------|-------------|
| `step` | integer | Counter value at capture time |
| `field` | string | Counter name (one of `code_task`, `review_step`, `learn_step`, `complete_step`) |
| _flattened snapshot fields_ | various | Every [Window Snapshot](#window-snapshot) field, inlined at the same level |

---

## Shared-Config Approval Markers

The shared-config gate (`validate_worktree_paths::validate_shared_config`)
blocks Edit/Write on `.gitignore`/`Cargo.toml`/`.github/`/etc. inside a
worktree. The "proceed" half is a branch-scoped, per-file, single-use
approval marker store at:

```text
.flow-states/<branch>/shared-config-approvals/<sha256(target_path)>
```

One marker file per approved target path. The on-disk filename is the
SHA-256 hex of the full target path string (collision-safe,
filesystem-safe — no separators or traversal segments). The marker body is:

```json
{"approved": true, "target": "/abs/path/to/Cargo.toml"}
```

Lifecycle:

- **Created** by `bin/flow approve-shared-config --path <file>` after the
  user replies with the exact line `approve shared-config: <path>`. The
  subcommand self-gates on the persisted transcript (the user-typed
  phrase is the unforgeable anchor — same trust model as `clear-halt`)
  before `shared_config_approval::write_approval` writes the marker.
- **Consumed** (read, validated, then deleted — single-use) by
  `validate_shared_config` immediately before its block return, allowing
  exactly one Edit/Write of exactly that file. Any unreadable, oversized
  (> 64 KB read cap), unparseable, wrong-root-type, `approved != true`,
  or target-mismatched marker yields no approval and the gate keeps
  blocking (fail-closed — a corrupt marker is never an escape hatch).
- **Bulk-cleared** by `shared_config_approval::clear_all` on phase
  advance (`phase_enter`), best-effort, so a stale approval cannot
  bleed into a later phase.

The directory is removed with the rest of `.flow-states/<branch>/` by
`/flow-complete` / `/flow-abort` cleanup.

---

## Merge-Approval Marker

When `flow-complete` is configured `manual`, the Phase 5 squash-merge
is gated: it must not run without an explicit user confirmation. The
"proceed" half is a branch-scoped, single-use approval marker at:

```text
.flow-states/<branch>/merge-approval
```

One marker file per branch directory. The marker body is:

```json
{"approved": true, "branch": "<branch>"}
```

Lifecycle:

- **Created** by `bin/flow confirm-merge --branch <branch>`, which the
  `flow-complete` skill invokes on the user's "Yes, merge" answer in
  Step 4. The external `--branch` string reaches the `.flow-states/`
  path only through `FlowPaths::try_new`, which rejects empty / `.` /
  `..` / `/`-bearing / NUL-bearing branches.
- **Consumed** (read, validated, then deleted — single-use) by both
  merge surfaces — `complete-merge` and `complete-fast` — before the
  freshness check that precedes the squash-merge. Because consumption
  runs before the freshness check, every merge attempt consumes the
  marker: a freshness outcome that loops back without merging
  (`ci_rerun`/`ci_stale`) still requires a fresh confirmation on the
  next attempt. Any unreadable, oversized (> 64 KB read cap),
  unparseable, wrong-root-type, `approved != true`, or
  branch-mismatched marker yields no approval and the merge stays
  refused with `{"status":"error","reason":"merge_not_confirmed"}`
  (fail-closed — a corrupt marker is never an escape hatch).

The marker is removed with the rest of `.flow-states/<branch>/` by
`/flow-complete` / `/flow-abort` cleanup.

---

## State Machine

Valid phase transitions are defined in `flow-phases.json` at the plugin root. Forward progression is always valid. Backward transitions are limited per phase.

Valid transitions are defined in `flow-phases.json`: Review can return to Code.
