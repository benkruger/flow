---
title: /flow-code-review
nav_order: 8
parent: Skills
---

# /flow-code-review

**Phase:** 4 — Code Review

**Usage:** `/flow-code-review`, `/flow-code-review --auto`, or `/flow-code-review --manual`

Three review lenses (clarity, correctness, safety) plus an optional fourth
(CLAUDE.md compliance via the code-review:code-review plugin). Combines
clarity review, code review, and security review into a single phase with
up to four ordered steps, each with its own commit checkpoint.

---

## Steps

### Step 1 — Simplify (clarity)

Performs three inline review passes sequentially (code reuse, code
quality, efficiency) against the branch diff. If changes are proposed,
shows the diff, commits via `/flow-commit`, and runs `bin/flow ci`. If
no changes, skips to Step 2.

### Step 2 — Review (correctness)

Performs an inline correctness review of the branch diff using five review
passes: plan alignment, logic correctness, test coverage, API contracts,
and rule compliance. Uses the plan file as context. If no findings, skips
to the next step.
Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 3 — Security (safety)

Performs an inline security review of the branch diff using three security
lenses: input validation, authentication and authorization, and data
exposure. If no findings, skips to the next step. Every finding is fixed,
`bin/flow ci` is run, and changes are committed via `/flow-commit`.

### Step 4 — Code Review Plugin (CLAUDE.md compliance, configurable)

Controlled by the `code_review_plugin` config axis. When set to `"never"`,
this step is skipped and the phase completes after Step 3.

When enabled (`"always"` or `"auto"`), invokes the `code-review:code-review`
plugin for multi-agent validation. Four parallel agents (2x CLAUDE.md
compliance, 1x bug scan, 1x security/logic scan) with a validation layer
that filters false positives. Waits for all background agents to complete
before evaluating findings. If no findings, skips to Done. Every finding is
fixed, `bin/flow ci` is run, and changes are committed via `/flow-commit`.

---

## Out-of-Scope Findings

Each finding is classified before fixing:

- **In-scope** — related to the feature, fixed as normal
- **Tech Debt** — pre-existing, unrelated. Filed as a "Tech Debt" issue via `bin/flow issue`, recorded via `bin/flow add-issue`, then skipped
- **Documentation Drift** — stale docs, unrelated. Filed as a "Documentation Drift" issue, recorded, then skipped

---

## Mode

Mode is configurable via `.flow.json` (default: manual). Three axes are
configurable independently:

- **commit** — `"auto"` or `"manual"` (default). Controls diff approval.
- **continue** — `"auto"` or `"manual"` (default). Controls phase advancement.
- **code\_review\_plugin** — `"always"` (default), `"auto"`, or `"never"`.
  Controls whether Step 4 (the code-review:code-review plugin) runs.

In auto mode, findings are auto-fixed and the phase transition advances to
Learn without asking. When `code_review_plugin` is `"never"`, the phase
completes after Step 3.

---

## Step Advancement

Steps advance via self-invocation: after each step completes, the skill
invokes itself with `--continue-step` as its final action. This prevents
context loss that occurs when the model treats a built-in skill return as
a conversation turn boundary. The `--continue-step` flag skips the
Announce banner and phase entry update, proceeding directly to the Resume
Check which dispatches to the next step.

Steps 1-3 perform inline review passes sequentially within the response
turn. Step 4 invokes the code-review plugin which may launch background
agents — it waits for all background agents to complete before evaluating
findings.

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after every fix in every step
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code or Plan
