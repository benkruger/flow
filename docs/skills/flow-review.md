---
title: /flow-review
nav_order: 8
parent: Skills
---

# /flow-review

**Phase:** 3 — Review

**Usage:** `/flow-review`, `/flow-review --auto`, or `/flow-review --manual`

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

Collect all artifacts. The diff is captured to canonical file paths
under `.flow-states/<branch>/` via `bin/flow capture-diff --branch
<branch> --base <base_branch>`, which writes both the full diff
(`full-diff.diff`) and the substantive diff (`substantive-diff.diff`,
whitespace-only changes filtered via `git diff -w`). Agents receive
the diff via file handoff (`DIFF_FILE` / `SUBSTANTIVE_DIFF_FILE`) and
Read the bytes themselves, keeping the parent skill's prompt budget
bounded as PR size grows. Step 1 also collects: plan file, CLAUDE.md,
rules files, the adversarial agent's probe path resolved by shelling
out to `bin/test --adversarial-path` (the path lives inside the
project's test tree so the language runner can discover it; halt on
exit 2 from an unconfigured stub), the `bin/flow ci --test --file`
runner command, and a narrowed list of doc paths likely affected by
the diff (derived from `git diff --name-only` via filename
heuristics — passed to the documentation agent in Step 2 so it
investigates only those paths instead of the full docs tree). Run
`tombstone-audit` to identify stale tombstones for removal in Step 4.
No analysis.

### Step 2 — Launch

Launch four agents in parallel. Reviewer is context-rich (receives
`DIFF_FILE`, plan, CLAUDE.md, rules). Pre-mortem and adversarial are
context-sparse (receive `SUBSTANTIVE_DIFF_FILE` only, investigate
independently). Documentation is context-sparse + narrowed: receives
`SUBSTANTIVE_DIFF_FILE` plus a filename-heuristic-derived list of doc
paths likely affected by the diff (Step 1 derives the list from
`git diff --name-only`), investigating only those paths so its turn
budget stays bounded on moderately-sized PRs.

All four `Agent` launches go in a single response with no intervening
tool call — no Bash, Read, Grep, Skill, or fifth `Agent` call —
between the first agent's launch and the fourth agent's return.
Per-agent classify-and-record calls (`record-agent-return`,
`add-skipped-agent`) run only after all four agents have returned;
interleaving them between launches serializes the four agents instead
of running them as one concurrent batch.

After agents return, the skill classifies each response in priority
order: truncation first (re-invoke with narrower partition), external
failure second (record via `bin/flow add-skipped-agent` with one of
`rate_limit`, `api_error`, `other`), normal completion otherwise.
When any agent is recorded as skipped, `phase-finalize` refuses to
advance until the user passes `--accept-skipped-agents` to
acknowledge the partial coverage.

After agents return, the skill checks each high-investigation agent
(reviewer, learn-analyst, documentation) for the literal
`END-OF-FINDINGS` completion marker. Marker absence means the agent
was truncated by `maxTurns` exhaustion; the skill re-invokes that
agent with a narrower partition (file family for documentation,
tenant family for reviewer/learn-analyst) and combines findings
across the multiple invocations. See
`.claude/rules/cognitive-isolation.md` "Context Budget + Truncation
Recovery".

### Step 3 — Triage

Classify each finding as **Real** (fix) or **False positive**
(dismiss with rationale). Each dismissal is recorded via
`bin/flow add-finding --outcome "dismissed"`. Shows triage summary
table. The supersession test from `.claude/rules/supersession.md`
runs before classification — code the PR has made permanently
redundant is routed to Step 4 for deletion regardless of file
location.

There is no filing path. All real findings are fixed in Step 4 —
see `.claude/rules/review-scope.md`. `bin/flow add-finding`
rejects `--outcome filed` during Review, and `bin/flow issue`
refuses to create issues while `current_phase == "flow-review"`
unless `--override-review-ban` is passed.

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

- Code phase must be complete before Review can start
- `bin/flow ci` must be green after all fixes
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code
- If any agent is recorded as skipped (`rate_limit`, `api_error`, or `other`), `bin/flow phase-finalize` refuses to advance until `--accept-skipped-agents` is passed. The skipped entries remain in state for the Learn-phase audit. See `docs/reference/flow-state-schema.md` "Agents-Skipped Gate" for the JSON contract.
