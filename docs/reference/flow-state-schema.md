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
  "started_at": "2026-02-20T10:00:00Z",
  "current_phase": 2,
  "framework": "rails",
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
| `framework` | string | `"rails"` or `"python"` — set during `/flow:init`, copied to state by `/flow:start` |
| `mode` | string / absent | `"light"` for light mode features, absent for standard mode |
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
| `skipped` | boolean / absent | `true` when the phase was skipped (light mode Design). Absent otherwise |

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
and session restarts. Read by Reflect as a primary source.

```json
"notes": [
  {
    "phase": 5,
    "phase_name": "Code",
    "timestamp": "2026-02-20T14:23:00Z",
    "type": "correction",
    "note": "Never assume branch-behind is unlikely — multiple active sessions means branches regularly fall behind main"
  }
]
```

---

## Plan Object

Added to the state file progressively as Plan sections are approved.
Section names are framework-defined (Rails uses `schema`, `models`,
`workers`, `controllers`, `integration`; Python uses `modules`, `scripts`,
`tests`).

```json
"plan": {
  "sections": {
    "schema":      { "status": "approved", "tasks": [1] },
    "models":      { "status": "approved", "tasks": [2,3,4,5] },
    "workers":     { "status": "skipped",  "tasks": [] },
    "controllers": { "status": "pending",  "tasks": [] },
    "integration": { "status": "pending",  "tasks": [] }
  },
  "current_section": "controllers",
  "tasks": [
    {
      "id": 1,
      "section": "schema",
      "type": "schema",
      "description": "Add payments table to data/release.sql",
      "files": ["data/release.sql"],
      "tdd": false,
      "status": "pending"
    }
  ],
  "approved_at": null
}
```

Section statuses: `pending`, `approved`, `skipped`
Task statuses: `pending`, `in_progress`, `complete` (updated by Code)

---

## Security Object

Added to the state file when Phase 7: Security completes its scan.

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
  "scanned_at": "2026-02-20T15:00:00Z"
}
```

Finding statuses: `pending`, `fixed`

`clean_checks` lists the check keys that found no issues. `scanned_at` is when the scan completed.

---

## Design Object

Added to the state file when Phase 3: Design completes. The change category
fields are framework-defined (Rails uses `schema_changes`, `model_changes`,
`controller_changes`, `worker_changes`, `route_changes`; Python uses
`module_changes`, `test_changes`, `script_changes`).

```json
"design": {
  "feature_description": "User's own words describing what they're building",
  "chosen_approach": "Approach title",
  "rationale": "Why this approach was chosen over the alternatives",
  "schema_changes": ["Add payments table with stripe_id column"],
  "model_changes": ["Payment::Base / Payment::Create split, belongs_to :account"],
  "controller_changes": ["New POST /api/webhooks/payment endpoint"],
  "worker_changes": ["PaymentWebhookWorker on critical queue"],
  "route_changes": ["api.rb: post 'webhooks/payment'"],
  "risks": ["before_save on Payment::Base sets processed_at from Current"],
  "approved_at": "2026-02-20T11:00:00Z"
}
```

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
