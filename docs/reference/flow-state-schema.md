---
title: FLOW State Schema
nav_order: 11
parent: Reference
---

# FLOW State Schema

State files live in `.claude/flow-states/` at the project root, named after the branch:

```
.claude/flow-states/app-payment-webhooks.json
.claude/flow-states/user-profile-redesign.json
```

One file per active feature. Multiple features can run simultaneously with no conflicts. The directory and all its contents are gitignored (covered by the global Claude gitignore on `.claude/`). Created by `/flow:start`, deleted by `/flow:cleanup`.

---

## Full Schema

```json
{
  "feature": "App Payment Webhooks",
  "branch": "app-payment-webhooks",
  "worktree": ".worktrees/app-payment-webhooks",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "started_at": "2026-02-20T10:00:00Z",
  "current_phase": 2,
  "phases": {
    "1": {
      "name": "Start",
      "status": "complete",
      "started_at": "2026-02-20T10:00:00Z",
      "completed_at": "2026-02-20T10:05:00Z",
      "session_started_at": null,
      "cumulative_seconds": 300,
      "visit_count": 1
    },
    "2": {
      "name": "Research",
      "status": "in_progress",
      "started_at": "2026-02-20T10:05:00Z",
      "completed_at": null,
      "session_started_at": "2026-02-20T10:30:00Z",
      "cumulative_seconds": 1800,
      "visit_count": 2
    },
    "3": {
      "name": "Design",
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
| `current_phase` | integer | The currently active phase number |

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

## Research Object

Added to the state file when Phase 2: Research completes. Extended if Research is revisited.

```json
"research": {
  "clarifications": [
    { "question": "What happens to existing webhooks when...", "answer": "They should..." }
  ],
  "affected_files": [
    "app/models/payment/base.rb",
    "app/workers/payment_webhook_worker.rb"
  ],
  "risks": [
    "Payment::Base has a before_save callback that sets processed_at from Current"
  ],
  "open_questions": [
    "Stripe webhook signing secret — confirmed in ENV but not yet in credentials"
  ],
  "summary": "Plain English summary of what was found."
}
```

---

## State Machine

Valid phase transitions are defined in `flow-phases.json` at the plugin root. Forward progression is always valid. Backward transitions are limited per phase.

See [Phase Comparison Reference](phase-comparison.md) for the full transition map.
