# Code Review Scope — All Real Findings Fixed In PR

## The Rule

Every real Code Review finding is fixed during Step 4. Triage has
two outcomes:

- **Real** → fix in Step 4
- **False positive** → dismiss with specific rationale citing code

There is no filing path. Filing a real finding as an out-of-scope
issue is not an option.

## Why

Filing a real finding is effort optimization dressed up as scope
discipline. Fixing now costs less than filing, triaging later, and
running a separate lifecycle on it. The current session has full
context; a future session starts from zero.

Mechanical enforcement ensures the path is absent:

- `bin/flow add-finding` applies a positive allowlist: the outcome
  must be in `{fixed, dismissed}` when `--phase flow-code-review`.
  Both inputs are normalized (whitespace trimmed, NULs stripped,
  ASCII-lowercased) before comparison, so whitespace or case drift
  in CLI arguments cannot bypass the gate. Any outcome outside the
  allowed set — including `filed` and any future outcome added to
  `VALID_OUTCOMES` — is rejected.
- `bin/flow issue` blocks issue creation when the state file shows
  `current_phase == "flow-code-review"` (normalized via the same
  trim + NUL-strip + lowercase). The gate fails CLOSED when a
  non-empty state file exists but its `current_phase` cannot be
  determined (invalid JSON, BOM prefix, wrong type, missing key).
  The `--override-code-review-ban` flag bypasses the gate and is
  the deliberate-friction escape hatch for exceptional cases
  (e.g., a FLOW process gap raised inside a Code Review that
  genuinely cannot wait for Phase 5 Learn).

## Supersession Exception

Before classifying a finding as Real or False positive, run the
supersession test from `.claude/rules/supersession.md`. If the
finding describes code the PR has made permanently redundant, the
in-scope action is deletion regardless of file location — not
filing. The supersession check is complementary to this rule; it
routes superseded code to Step 4 for deletion.

## New Rules Added Alongside Code

When a PR adds a new `.claude/rules/*.md` file that retroactively
flags pre-existing violations, the pre-existing violations are
still Real findings and still get fixed in Step 4. A new rule
without a sweep of the codebase is incomplete — see
`.claude/rules/scope-expansion.md` for the decision tree.

## Rules Landed on Main Mid-Flow

The same retroactive-fix discipline applies when a rule update lands
on **main** during an active Code or Code Review phase on an
already-started branch. Rule updates flow into the current session
via the auto-inserted `system-reminder` that surfaces edited rule
files — the Code phase sees the updated rule text even though the
feature branch forked before it was written.

When this happens, the Code phase has a decision to make:

1. **Proactively sweep the files the branch is already modifying**
   for pre-existing violations of the new rule, or
2. **Defer the sweep to Code Review**, where the Reviewer and
   Adversarial agents will catch the same violations under the new
   rule's lens.

### Decision criteria

Take the proactive sweep path when the new rule's violation class is:

- **Security-sensitive** — panics on untrusted input, missing
  auth/authz checks, data exposure, injection surfaces. Cost of
  deferring is a potential production incident.
- **Adjacent to already-changed code** — the rule flags code on the
  same function, file, or module the current task is already
  touching. Sweeping is nearly free; deferring just moves the same
  edit to a later phase.
- **Cheap to verify** — the rule has a mechanical enforcer
  (`bin/flow plan-check`, `tests/*.rs` contract test, hook) that
  will run during `bin/flow ci` and immediately surface the
  violation.

Defer to Code Review when the new rule's violation class is:

- **Incidental** — style, documentation shape, comment quality.
- **Wide-blast-radius** — the rule flags code across many files the
  current PR does not touch, and sweeping would balloon scope.
- **Still being refined** — the rule file's commit history shows
  recent churn, suggesting the wording is not yet stable enough to
  build structural guards around.

### Logging the decision

**Whichever path you take, log the decision** via
`bin/flow log <branch> "[Phase N] Rule drift: <rule file> landed on
main. Decision: <proactive sweep | defer to Code Review>. Reason:
<criterion>"`. The log entry is what distinguishes "Claude noticed
the rule and consciously chose a path" from "Claude ignored the
rule". The Learn phase analyst reads the log when auditing rule
compliance and treats an undocumented decision as a process gap.

### Motivating incident

PR #1157 (Coverage: HTTP client trait seam) was mid-Code-phase when
`.claude/rules/external-input-validation.md` was updated on main to
extend the `FlowPaths::try_new` discipline to CLI subcommand
`--branch` overrides (per issue #1137). The Code phase notes
indicated awareness of the rule update but elected not to fix the
pre-existing `FlowPaths::new(root, branch)` call in
`phase_finalize::run_impl_with_deps`. Code Review's reviewer agent
caught the violation under the new rule's lens, and the fix landed
as a Code Review finding in the same PR. The deferral was a valid
scope-management decision per this rule's "defer" criterion
(wide-blast-radius is arguable; the call was adjacent to already-
changed code, so "proactively sweep" would also have been
defensible). What was missing was the log entry — the decision was
implicit, leaving Learn to infer the reasoning from commit messages
and absent state notes. This section codifies the logging
discipline.
