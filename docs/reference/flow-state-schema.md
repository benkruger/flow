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

Each feature has up to four files: the state file (`.json`), the log file (`.log`), a frozen copy of `flow-phases.json` (`-phases.json`), and a CI sentinel (`-ci-passed`). The CI sentinel caches the last passing `bin/flow ci` snapshot to skip redundant runs with `--if-dirty`. Multiple features can run simultaneously with no conflicts. The directory is added to `.git/info/exclude` by `/flow-start` (per-repo, not committed). Created by `/flow-start`, deleted by `/flow-complete`.

The frozen phases file is a snapshot of `flow-phases.json` taken at start time. Scripts use it instead of the live plugin source so that phase config changes during FLOW development don't break in-progress features.

---

## Full Schema

```json
{
  "feature": "App Payment Webhooks",
  "branch": "app-payment-webhooks",
  "worktree": ".worktrees/app-payment-webhooks",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "started_at": "2026-02-20T10:00:00-08:00",
  "current_phase": "flow-plan",
  "framework": "rails",
  "prompt": "fix #83 and #89 — close issues at complete time",
  "plan_file": null,
  "session_id": null,
  "transcript_path": null,
  "skills": {
    "flow-start": {"continue": "manual"},
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
  }
}
```

---

## Top-Level Fields

| Field | Type | Description |
|-------|------|-------------|
| `feature` | string | Human-readable feature name — may be long |
| `branch` | string | Git branch name — slug format |
| `worktree` | string | Path to the git worktree |
| `pr_number` | integer | GitHub PR number |
| `pr_url` | string | Full GitHub PR URL |
| `started_at` | ISO 8601 | When the feature was started (Phase 1 entry) |
| `current_phase` | string | The currently active phase key (e.g. `"flow-code"`) |
| `framework` | string | `"rails"` or `"python"` — set during `/flow-prime`, copied to state by `/flow-start` |
| `plan_file` | string / null | Absolute path to the plan file at `~/.claude/plans/<name>.md` — set by Phase 2: Plan |
| `session_id` | string / null | Claude Code session UUID — set by Stop hook from hook stdin |
| `transcript_path` | string / null | Absolute path to session transcript .jsonl — set by Stop hook from hook stdin |
| `skills` | object / absent | Per-skill autonomy settings copied from `.flow.json` by `/flow-start` — see [Skills Object](#skills-object) |
| `code_review_step` | integer | Last completed Code Review step (0-4). Set to 0 on phase entry, incremented after each step. Used for resume after context compaction. |
| `_continue_pending` | string | Name of the child skill currently executing. Set before invoking a child skill, cleared by the Stop hook after forcing continuation. Empty string or absent means no continuation pending. |
| `prompt` | string | The full text passed to `/flow-start` — used by Plan as feature description and by Complete to extract `#N` issue references for auto-closing |
| `notes` | array | Corrections captured via `/flow-note` — see [Notes Array](#notes-array) |

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
  "flow-code-review": {"commit": "auto", "continue": "auto"},
  "flow-learn": {"commit": "auto", "continue": "auto"},
  "flow-abort": "auto",
  "flow-complete": "auto"
}
```

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

## Plan File

The plan lives outside the state file at `~/.claude/plans/<name>.md` (Claude Code's native plan file location). The state file stores only the path in `plan_file`. The plan file includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## Security Object

Added to the state file when the Security step of Phase 4: Code Review completes its scan.

```json
"security": {
  "findings": [
    {
      "id": 1,
      "check": "authorization_gaps",
      "description": "PaymentController#show has no before_action auth check",
      "file": "app/controllers/payment_controller.rb",
      "line": 15,
      "status": "pending"
    }
  ],
  "clean_checks": ["sql_injection", "csrf_bypass", "open_redirects"],
  "scanned_at": "2026-02-20T15:00:00-08:00"
}
```

Finding statuses: `pending`, `fixed`

`clean_checks` lists the check keys that found no issues. `scanned_at` is when the scan completed.

---

## State Machine

Valid phase transitions are defined in `flow-phases.json` at the plugin root. Forward progression is always valid. Backward transitions are limited per phase.

See [Phase Comparison Reference](phase-comparison.md) for the full transition map.
