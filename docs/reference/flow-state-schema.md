---
title: FLOW State Schema
nav_order: 11
parent: Reference
---

# FLOW State Schema

State files live in `.flow-states/` at the project root, named after the branch:

```text
.flow-states/app-payment-webhooks.json
.flow-states/user-profile-redesign.json
```

One file per active feature. Multiple features can run simultaneously with no conflicts. The directory is added to `.git/info/exclude` by `/flow:start` (per-repo, not committed). Created by `/flow:start`, deleted by `/flow:cleanup`.

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
  "current_phase": "plan",
  "framework": "rails",
  "plan_file": null,
  "phases": {
    "start": {
      "name": "Start",
      "status": "complete",
      "started_at": "2026-02-20T10:00:00-08:00",
      "completed_at": "2026-02-20T10:05:00-08:00",
      "session_started_at": null,
      "cumulative_seconds": 300,
      "visit_count": 1
    },
    "plan": {
      "name": "Plan",
      "status": "in_progress",
      "started_at": "2026-02-20T10:05:00-08:00",
      "completed_at": null,
      "session_started_at": "2026-02-20T10:30:00-08:00",
      "cumulative_seconds": 1800,
      "visit_count": 2
    },
    "code": {
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
| `current_phase` | string | The currently active phase key (e.g. `"code"`) |
| `framework` | string | `"rails"` or `"python"` — set during `/flow:init`, copied to state by `/flow:start` |
| `plan_file` | string / null | Absolute path to the plan file at `~/.claude/plans/<name>.md` — set by Phase 2: Plan |
| `notes` | array | Corrections captured via `/flow:note` — see [Notes Array](#notes-array) |

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

## Notes Array

Populated throughout the session by `/flow:note`. Survives compaction
and session restarts. Read by Learning as a primary source.

```json
"notes": [
  {
    "phase": "code",
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

Added to the state file when Phase 5: Security completes its scan.

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
