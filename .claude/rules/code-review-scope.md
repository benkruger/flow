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
discipline. A real finding sits in a file the current PR either
created, modified, or surfaces through its changes — fixing it now
costs less than filing, triaging later, and running a separate
lifecycle on it. The current session has full context; a future
session starts from zero.

Mechanical enforcement ensures the path is absent:

- `bin/flow add-finding` rejects `--outcome filed` when
  `--phase flow-code-review` is set.
- `bin/flow issue` refuses to create issues while
  `current_phase == "flow-code-review"` unless
  `--override-code-review-ban` is passed.

## Supersession Exception

Before classifying a finding as Real or False positive, run the
supersession test from `.claude/rules/supersession.md`. If the
finding describes code the PR has made permanently redundant, the
in-scope action is deletion regardless of file location — not
filing. The supersession check is complementary to this rule; it
routes superseded code to Step 4 for deletion.

## New Rules Introduced by This PR

When this PR adds a new `.claude/rules/*.md` file that retroactively
flags pre-existing violations, the pre-existing violations are
still Real findings and still get fixed in Step 4. A new rule that
the PR introduces without sweeping the codebase is incomplete —
see `.claude/rules/scope-expansion.md` for the decision tree.
