---
title: /flow-code-review
nav_order: 8
parent: Skills
---

# /flow-code-review

**Phase:** 4 — Code Review

**Usage:** `/flow-code-review`, `/flow-code-review --auto`, or `/flow-code-review --manual`

Six tenants assessed by four cognitively isolated agents (reviewer,
pre-mortem, adversarial, documentation) launched in parallel. The parent
session gathers context, triages findings, and fixes. All analysis comes
from agents — the parent session never reviews the diff itself.

---

## Six Tenants

1. Architecture — conventions, rules, plan alignment
2. Simplicity — unnecessary complexity, duplication
3. Maintainability — comprehension barriers for newcomers
4. Correctness — logic errors, edge cases, security
5. Test coverage — proven gaps via adversarial tests
6. Documentation — drift between docs and code behavior

---

## Steps

### Step 1 — Gather

Collect all artifacts: full branch diff, substantive diff (whitespace
changes filtered via `git diff -w`), plan file, CLAUDE.md, rules files,
parameterize the adversarial agent's temp test file and `bin/flow ci
--test --file` runner command, run `tombstone-audit` to identify stale
tombstones for removal in Step 4. No analysis.

### Step 2 — Launch

Launch four agents in parallel. Reviewer is context-rich (receives full
diff, plan, CLAUDE.md, rules). Pre-mortem, adversarial, and documentation
are context-sparse (receive substantive diff only, investigate
independently).

### Step 3 — Triage

Classify each finding as **Real** (fix) or **False positive**
(dismiss with rationale). Each dismissal is recorded via
`bin/flow add-finding --outcome "dismissed"`. Shows triage summary
table. The supersession test from `.claude/rules/supersession.md`
runs before classification — code the PR has made permanently
redundant is routed to Step 4 for deletion regardless of file
location.

There is no filing path. All real findings are fixed in Step 4 —
see `.claude/rules/code-review-scope.md`. `bin/flow add-finding`
rejects `--outcome filed` during Code Review, and `bin/flow issue`
refuses to create issues while `current_phase == "flow-code-review"`
unless `--override-code-review-ban` is passed.

### Step 4 — Fix

Fix all real findings, recording each fix via `bin/flow add-finding`.
Run `bin/flow ci`, commit once via `/flow-commit`.

---

## Mode

Mode is configurable via `.flow.json` (default: manual). Two axes are
configurable independently:

- **commit** — `"auto"` or `"manual"` (default). Controls per-task review before committing.
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

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after all fixes
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code or Plan
