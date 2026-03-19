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

Each feature has up to four files: the state file (`.json`), the log file (`.log`), a frozen copy of `flow-phases.json` (`-phases.json`), and a CI sentinel (`-ci-passed`). The CI sentinel caches the last passing `bin/flow ci` snapshot so subsequent runs skip automatically when nothing changed (use `--force` to bypass). Multiple features can run simultaneously with no conflicts. The directory is added to `.git/info/exclude` by `/flow-start` (per-repo, not committed). Created by `/flow-start`, deleted by `/flow-complete`.

**State files are local to each machine.** In a multi-engineer team, each engineer's `.flow-states/` directory only contains their own features. GitHub (issues, PRs, labels) is the shared coordination layer visible to all engineers. The "Flow In-Progress" label on issues is the mechanism for cross-engineer WIP detection — see `/flow-issues`.

The frozen phases file is a snapshot of `flow-phases.json` taken at start time. Scripts use it instead of the live plugin source so that phase config changes during FLOW development don't break in-progress features.

---

## Full Schema

```json
{
  "schema_version": 1,
  "branch": "app-payment-webhooks",
  "repo": "org/repo",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "started_at": "2026-02-20T10:00:00-08:00",
  "current_phase": "flow-plan",
  "framework": "rails",
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
    "flow-code-review": {"commit": "auto", "continue": "auto", "code_review_plugin": "always"},
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
  "issues_filed": []
}
```

---

## Top-Level Fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | integer | Schema version marker — currently `1` |
| `branch` | string | Git branch name — slug format. Canonical identity field. Feature name and worktree path are derived from this at read time |
| `repo` | string / null | GitHub repo in `owner/repo` format, cached during `/flow-start`. Used by `issue.py` to avoid repeated `git remote` calls. Null if detection fails |
| `pr_number` | integer | GitHub PR number |
| `pr_url` | string | Full GitHub PR URL |
| `started_at` | ISO 8601 | When the feature was started (Phase 1 entry) |
| `current_phase` | string | The currently active phase key (e.g. `"flow-code"`) |
| `framework` | string | `"rails"`, `"python"`, or `"ios"` — set during `/flow-prime`, copied to state by `/flow-start` |
| `files` | object | Structured artifact file paths — see [Files Object](#files-object) |
| `plan_file` | string / null | Legacy: absolute path to the plan file. Superseded by `files.plan` — kept for backward compatibility |
| `session_id` | string / null | Claude Code session UUID — set by Stop hook from hook stdin |
| `transcript_path` | string / null | Absolute path to session transcript .jsonl — set by Stop hook from hook stdin |
| `skills` | object / absent | Per-skill autonomy settings copied from `.flow.json` by `/flow-start` — see [Skills Object](#skills-object) |
| `code_review_step` | integer | Last completed Code Review step (0-4). Set to 0 on phase entry, incremented after each step. Used for resume after context compaction. |
| `_continue_pending` | string | Child skill or action currently executing. Phase skills set this before invoking a child skill so the Stop hook (`stop-continue.py`) blocks the turn from ending and forces continuation. Values are either a child skill name (`review`, `security-review`, `code-review:code-review`, `local-permission`) or the action `commit` (used by flow-code, flow-code-review, flow-learn, and flow-complete when invoking `/flow:flow-commit`). Cleared by the Stop hook after forcing continuation. Empty string or absent means no continuation pending. |
| `_continue_context` | string | Specific next-step instructions for the model after a child skill returns. Written by phase skills before `_continue_pending`, read and cleared by the Stop hook. Included in the block reason so the model knows what to do after the turn boundary. Empty string or absent means use the generic fallback message. |
| `_auto_continue` | string | Command to invoke next (e.g. `/flow:flow-plan`). Set by `phase_complete()` when `skills.<phase>.continue` is `"auto"`. Cleared by `phase_enter()` when the next phase starts. A PreToolUse hook on AskUserQuestion blocks prompts while this flag is set. |
| `prompt` | string | The full text passed to `/flow-start` — used by Plan as feature description and by Complete to extract `#N` issue references for auto-closing |
| `notes` | array | Corrections captured via `/flow-note` — see [Notes Array](#notes-array) |
| `phase_transitions` | array | Phase entry log recording every `phase_enter()` call with from/to/timestamp and optional reason — see [Phase Transitions Array](#phase-transitions-array) |
| `issues_filed` | array | GitHub issues filed during the feature — see [Issues Filed Array](#issues-filed-array) |
| `compact_summary` | string / null | Conversation summary from last compaction. Written by PostCompact hook, consumed and cleared by SessionStart hook. Transient. |
| `compact_cwd` | string / null | CWD at last compaction time. Written by PostCompact hook, consumed and cleared by SessionStart hook. Transient. |
| `compact_count` | integer | Total number of context compactions during this feature. Incremented by PostCompact hook. Permanent. |

---

## Phase Fields

Each phase entry has identical fields regardless of status.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Human-readable phase name |
| `status` | string | `pending`, `in_progress`, or `complete` |
| `started_at` | ISO 8601 / null | First time this phase was entered — **never overwritten** |
| `completed_at` | ISO 8601 / null | Most recent time this phase was exited — updated on every completion |
| `session_started_at` | ISO 8601 / null | Timestamp when current session entered this phase — reset if session interrupted |
| `cumulative_seconds` | integer | Total seconds spent in this phase across all visits — additive |
| `visit_count` | integer | Number of times this phase has been entered |

---

## Timing Rules

- `started_at` is set on first entry and **never changed again**
- `completed_at` is set on every exit — reflects the most recent completion
- `session_started_at` is set on entry and cleared to `null` on exit
- On session resume, if `session_started_at` is not null, it is reset to null — the interrupted visit's time is not counted
- `cumulative_seconds` increments by `(exit_time - session_started_at)` on each clean exit

---

## Skills Object

Copied from `.flow.json` into the state file by `/flow-start`. Phase skills read autonomy config from the state file rather than `.flow.json`, because `.flow.json` lives at the project root and is not accessible from worktrees.

Present only when `.flow.json` contains a `skills` key (i.e., after running `/flow-prime` with Customize or a preset). Phase skills that don't find a `skills` key in the state file fall back to built-in defaults.

```json
"skills": {
  "flow-start": {"continue": "manual"},
  "flow-code": {"commit": "manual", "continue": "manual"},
  "flow-code-review": {"commit": "auto", "continue": "auto", "code_review_plugin": "always"},
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

See [Phase Comparison Reference](phase-comparison.md) for the full transition map.
