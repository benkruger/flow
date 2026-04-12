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
