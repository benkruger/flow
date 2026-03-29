---
title: /flow-code-review
nav_order: 8
parent: Skills
---

# /flow-code-review

**Phase:** 4 — Code Review

**Usage:** `/flow-code-review`, `/flow-code-review --auto`, or `/flow-code-review --manual`

Five review steps — clarity with convention compliance, correctness with
rule compliance, safety, context-isolated code review, and pre-mortem
incident analysis. Combines inline review passes and two context-isolated
agents into a single phase with five ordered steps, each with its own
commit checkpoint.

---

## Steps

### Step 1 — Simplify (clarity + convention compliance)

Performs four inline review passes sequentially (code reuse, code
quality, efficiency, convention compliance) against the branch diff. If changes are proposed,
shows the diff, commits via `/flow-commit`, and runs `bin/flow ci`. If
no changes, skips to Step 2.

### Step 2 — Review (correctness)

Performs an inline correctness review of the branch diff using five review
passes: plan alignment, logic correctness, test coverage, API contracts,
and rule compliance. Uses the plan file as context. When the diff modifies
files containing step headings, the logic correctness pass also reads the
full resulting file to verify sequential step numbering and cross-reference
consistency. If no findings, skips to the next step.
Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 3 — Security (safety)

Performs an inline security review of the branch diff using three security
lenses: input validation, authentication and authorization, and data
exposure. If no findings, skips to the next step. Every finding is fixed,
`bin/flow ci` is run, and changes are committed via `/flow-commit`.

### Step 4 — Context-Isolated Review (cold reviewer)

Launches the `reviewer` custom agent — a context-isolated sub-agent that
receives the branch diff, plan file, CLAUDE.md, and `.claude/rules/` but
no conversation history or coding rationale. The agent reviews as a cold
reviewer: "You are reviewing code you did not write."

The agent produces structured findings (severity, category, evidence,
recommendation). The main session triages each finding as real or false
positive. Real findings are fixed, `bin/flow ci` is run, and changes are
committed via `/flow-commit`.

### Step 5 — Pre-Mortem (incident analysis)

Launches the `pre-mortem` custom agent — a context-isolated sub-agent that
receives only the branch diff and codebase access, with no conversation
history or coding rationale. The agent frames the review as an incident
investigation: "This PR was merged and caused a production incident."

The agent produces a structured incident report (root cause hypothesis,
blast radius, what tests missed, severity, evidence). The main session
triages each finding as real or false positive. Real findings are fixed,
`bin/flow ci` is run, and changes are committed via `/flow-commit`.

---

## Out-of-Scope Findings

Each finding is classified before fixing:

- **In-scope** — related to the feature, fixed as normal
- **Tech Debt** — pre-existing, unrelated. Filed as a "Tech Debt" issue via `bin/flow issue`, recorded via `bin/flow add-issue`, then skipped
- **Documentation Drift** — stale docs, unrelated. Filed as a "Documentation Drift" issue, recorded, then skipped

---

## Mode

Mode is configurable via `.flow.json` (default: manual). Two axes are
configurable independently:

- **commit** — `"auto"` or `"manual"` (default). Controls diff approval.
- **continue** — `"auto"` or `"manual"` (default). Controls phase advancement.

In auto mode, findings are auto-fixed and the phase transition advances to
Learn without asking.

---

## Step Advancement

Steps advance via self-invocation: after each step completes, the skill
invokes itself with `--continue-step` as its final action. This prevents
context loss that occurs when the model treats a built-in skill return as
a conversation turn boundary. The `--continue-step` flag skips the
Announce banner and phase entry update, proceeding directly to the Resume
Check which dispatches to the next step.

Steps 1-3 perform inline review passes sequentially within the response
turn. Step 4 launches the reviewer agent for context-isolated code review.
Step 5 launches the pre-mortem agent for context-isolated incident
analysis.

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after every fix in every step
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code or Plan
