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
  ASCII-lowercased) before comparison.
- `bin/flow issue` blocks issue creation when the state file shows
  `current_phase == "flow-code-review"`. The gate fails CLOSED
  when a non-empty state file exists but its `current_phase`
  cannot be determined. The `--override-code-review-ban` flag
  bypasses the gate and is the deliberate-friction escape hatch
  for exceptional cases.

## Supersession Exception

Before classifying a finding as Real or False positive, run the
supersession test from `.claude/rules/supersession.md`. If the
finding describes code the PR has made permanently redundant, the
in-scope action is deletion regardless of file location — not
filing.

## Value-vs-Bureaucracy Finding Triage

Agent findings are hypotheses, not verdicts. Every Real
classification must survive a value test before routing to Step 4:
"would the proposed fix add signal that an informed reader cannot
already derive from the code, the existing rule files, or the
diff?" If the answer is "no — the fix duplicates information
already discoverable through grep, rustdoc, the file's own doc
comments, or a sibling rule," the finding is a false positive
even when the agent's claim is technically correct.

The bureaucracy-trap shape: a finding flags a missing CLAUDE.md
"Key Files" entry for a small extracted helper whose purpose is
already stated in its module doc comment, a missing
cross-reference between two rules that already mention each
other in adjacent sections, or a missing redundant doc comment
that restates what the source already says. The fix lands quickly
and looks productive, but it does not change behavior, does not
unblock a future reader, and does not prevent any class of
regression.

### The triage test

For every Real candidate produced in Step 3, ask:

1. **Behavior.** Does the fix change runtime behavior, prevent a
   class of bug, or unblock a workflow? If yes, classify Real.
2. **Discoverability.** If the fix is a documentation update,
   would a reader using grep, rustdoc, the existing rule
   cross-references, or the file's own doc comments find the
   information without it? If yes, the proposed fix is
   redundant — classify False positive.
3. **Forward applicability.** Would the fix help a session that
   has not yet been written? A fix that only records work
   already complete in this PR — without changing how a future
   session would discover or apply that work — is record-keeping
   for record-keeping's sake. Classify False positive.
4. **Author-driven exception.** When in doubt, surface the
   question to the user rather than auto-routing to Step 4
   under the agent's framing. The user is the final arbiter on
   value calls.

### What this is NOT

The triage test is not permission to dismiss findings that touch
real behavior, security, correctness, or test coverage. Findings
in tenants 1–5 (architecture, simplicity, maintainability,
correctness, test coverage) almost always pass the value test
because their fixes change behavior or prevent bugs.

The trap concentrates in tenant 6 (documentation): doc-drift
findings that restate information already encoded elsewhere.
Apply the test most rigorously there.

The "Key Files" addition in `CLAUDE.md` is the canonical edge
case: per `.claude/rules/docs-with-behavior.md` "What Counts," a
*permanent on-main artifact* requires a Key Files entry. A small
helper extracted as part of a larger feature, when the helper's
purpose is already documented in its module doc comment AND in
the file that calls it, is borderline — apply the discoverability
and forward-applicability tests rigorously and surface the
decision to the user when uncertain.

### Cross-reference

`.claude/rules/forward-facing-authoring.md` "How Code Review
Applies This" governs the *form* of documentation fixes that pass
this triage. This rule governs whether a documentation fix should
be applied at all.

## New Rules Added Alongside Code

When a PR adds a new `.claude/rules/*.md` file that retroactively
flags pre-existing violations, the pre-existing violations are
still Real findings and still get fixed in Step 4. A new rule
without a sweep of the codebase is incomplete — see
`.claude/rules/scope-expansion.md` for the decision tree.

## Rules or Skills Landed on the Base Branch Mid-Flow

The same retroactive-fix discipline applies when a rule update OR a
skill update lands on **the base branch** (the integration branch
the flow coordinates against — `main` for standard repos,
`staging`/`develop`/etc. for non-main-trunk repos) during an active
Code or Code Review phase on an already-started branch. Both rule
files and skill files flow into the current session via the
auto-inserted `system-reminder` that surfaces edited files — the
Code phase sees the updated text even though the feature branch
forked before it was written.

Skills are the same drift surface as rules: both are dynamic
instructions the Code phase follows. A skill that adds a new step,
changes a gate, or tightens a commit convention can retroactively
flag the current branch's in-progress work.

When this happens, the Code phase has a decision to make:

1. **Proactively sweep the files the branch is already modifying**
   for pre-existing violations of the new rule or skill, or
2. **Defer the sweep to Code Review**, where the Reviewer and
   Adversarial agents will catch the same violations under the new
   rule's or skill's lens.

### Decision criteria

Take the proactive sweep path when the new rule's or skill's
violation class is:

- **Security-sensitive** — panics on untrusted input, missing
  auth/authz checks, data exposure, injection surfaces. Cost of
  deferring is a potential production incident.
- **Adjacent to already-changed code** — the rule or skill flags
  code on the same function, file, or module the current task is
  already touching. Sweeping is nearly free; deferring just moves
  the same edit to a later phase.
- **Cheap to verify** — the rule or skill has a mechanical enforcer
  (`bin/flow plan-check`, `tests/*.rs` contract test, hook) that
  will run during `bin/flow ci` and immediately surface the
  violation.

Defer to Code Review when the violation class is:

- **Incidental** — style, documentation shape, comment quality.
- **Wide-blast-radius** — the rule or skill flags code across many
  files the current PR does not touch, and sweeping would balloon
  scope.
- **Still being refined** — the file's commit history shows recent
  churn, suggesting the wording is not yet stable enough to build
  structural guards around.

### Logging the decision

**Whichever path you take, log the decision** via
`bin/flow log <branch> "[Phase N] <Rule | Skill> drift: <file> landed
on the base branch. Decision: <proactive sweep | defer to Code Review>.
Reason: <criterion>"`. The log entry is what distinguishes "Claude
noticed the change and consciously chose a path" from "Claude
ignored the change". The Learn phase analyst reads the log when
auditing compliance and treats an undocumented decision as a
process gap.
