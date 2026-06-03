---
title: "Phase 3: Review"
nav_order: 4
---

# Phase 3: Review

**Command:** `/flow-review`

Six tenants assessed by four cognitively isolated agents launched in
parallel. The parent session gathers context, triages findings, and
fixes. All analysis comes from agents — the parent session never reviews
the diff itself, eliminating the self-reporting bias of inline
self-review.

---

## Six Tenants

Every finding must map to one of these tenants:

1. **Architecture** — does the code follow the project's conventions?
2. **Simplicity** — is there unnecessary complexity?
3. **Maintainability** — can a newcomer understand this?
4. **Correctness** — logic errors, edge cases, security?
5. **Test coverage** — every production line exercised by a named test; any uncovered line is a Real finding
6. **Documentation** — do docs match the code after these changes?

---

## The Four Steps

### Step 1 — Gather

Collect all artifacts. The diff is captured to canonical file paths
under `.flow-states/<branch>/` via `bin/flow capture-diff` — both
full (`full-diff.diff`) and substantive (`substantive-diff.diff`,
whitespace-only changes filtered via `git diff -w`). Agents receive
the diff via file handoff (`DIFF_FILE` / `SUBSTANTIVE_DIFF_FILE`) and
Read the bytes themselves, keeping the parent skill's prompt budget
bounded as PR size grows. If `capture-diff` reports a missing base
ref (`origin/<base>` not fetched into the worktree), Step 1 runs a
single `git fetch origin <base>` and retries once, halting rather
than launching the agents with a missing diff. Step 1 also collects
the plan file, CLAUDE.md, `.claude/rules/` files, and checks that
`bin/flow ci --test` exists for adversarial testing.

### Step 2 — Launch

Launch four agents in parallel using multiple Agent tool calls in a
single response:

- **Reviewer** (context-rich): receives `DIFF_FILE`, plan, CLAUDE.md,
  rules. Covers architecture (T1), simplicity (T2), and correctness
  including security (T4).
- **Pre-mortem** (context-sparse): receives only `SUBSTANTIVE_DIFF_FILE`,
  investigates the codebase independently. Covers correctness failure
  modes including security (T4).
- **Adversarial** (context-sparse): receives `SUBSTANTIVE_DIFF_FILE`
  and writes tests designed to fail. Covers test coverage (T5).
  Always launched — if the project's `bin/test` does not support
  `--file <path>` for single-file execution, the agent surfaces that
  as a finding instead of silently skipping.
- **Documentation** (context-sparse): receives `SUBSTANTIVE_DIFF_FILE`
  and doc paths, investigates the codebase. Covers maintainability
  (T3) and documentation accuracy (T6).

All four `Agent` launches go in that single response with no
intervening tool call — no Bash, Read, Grep, Skill, or fifth `Agent`
call — between the first agent's launch and the fourth agent's
return. Each launch is recorded into
`phases.flow-review.agents_returned` by the `PreToolUse:Agent` hook
(`src/hooks/agent_run_record.rs`) — the Agent tool call itself is the
evidence the agent ran — so no per-agent record call runs between
launches.

After agents return, each response is classified in priority order:
truncation first (re-invoke with a narrower partition), external
failure second (re-invoke once, then note the failure and proceed),
normal completion otherwise. `phase-finalize` gates on
`phases.flow-review.agents_returned`: it refuses to advance with
`required_agent_not_returned` naming any required agent that was never
launched, and the recovery is to re-launch that agent.

### Step 3 — Triage

For each finding from all agents, classify as:

- **Real** — fix in Step 4
- **False positive** — dismiss with rationale citing code

There is no filing path. All real findings are fixed during Code
Review — see `.claude/rules/review-scope.md`. Mechanical
enforcement blocks filing: `bin/flow add-finding` rejects
`--outcome filed` for `--phase flow-review`, and `bin/flow issue`
refuses to create issues while `current_phase == "flow-review"`
unless `--override-review-ban` is passed.

The supersession test from `.claude/rules/supersession.md` runs
before classification — code the PR has made permanently redundant
is routed to Step 4 for deletion regardless of file location.

### Step 4 — Fix

Fix all real findings, run `bin/flow ci`, commit once.

---

## bin/flow ci Rule

`bin/flow ci` runs after all fixes in Step 4. Review does not
transition to Learn until `bin/flow ci` is green.

---

## Back Navigation

- **Go back to Code** — revert to Code phase

---

## What Comes Next

Phase 4: Learn (`/flow-learn`) — audit rule compliance and identify
process gaps before the PR is merged.
