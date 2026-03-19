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

Launches three foreground review agents in parallel (code reuse, code
quality, efficiency) against the branch diff. All three complete within
the response turn — no background agents. If changes are proposed,
shows the diff, commits via `/flow-commit`, and runs `bin/flow ci`. If
no changes, skips to Step 2.

### Step 2 — Review (correctness)

Invokes Claude Code's built-in `/review` against the PR. Waits for all
background agents to complete before evaluating findings. Checks plan
alignment, risk coverage, and framework anti-patterns. If no findings,
skips to the next step. Every finding is fixed, `bin/flow ci` is run,
and changes are committed via `/flow-commit`.

### Step 3 — Security (safety)

Invokes Claude Code's built-in `/security-review` against the PR diff.
Waits for all background agents to complete before evaluating findings.
If no findings, skips to the next step. Every finding is fixed,
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

Step 1 uses foreground agents that complete within the response turn.
Steps 2-4 invoke built-in skills or plugins that may launch background
review agents — each of those steps waits for all background agents to
complete before evaluating findings.

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after every fix in every step
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code or Plan
