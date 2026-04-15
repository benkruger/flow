# Duplicate Test Coverage

When a Plan-phase plan proposes a new test function whose name
normalizes to the same identifier as an existing test in the suite,
the Plan-phase gate (`bin/flow plan-check`) flags the proposal. The
author either renames the new test, confirms the duplicate is
intentional via an opt-out comment, or deletes the new test in favor
of strengthening the existing one.

## Why

Tests earn their place in the suite by guarding a specific named
regression. A second test with the same normalized name typically
exercises the same production path — catching the same regression
twice is pure cost. The
`.claude/rules/tests-guard-real-regressions.md` "Forbidden patterns"
section calls this out as a duplicate guard, and the cheapest catch
is at Plan time before the second test is written.

Issue #1175 / PR #1173 surfaced this. The plan named
`stop_continue_qa_pending_fallback_blocks` as a new subprocess
test; a pre-existing `test_stop_continue_qa_pending_fallback_blocks`
(identical except for the `test_` prefix) already exercised the same
production path. Code Review caught the duplicate and deleted the
older test, costing a full review cycle on work the Plan-phase gate
could have prevented.

## The Rule

The scanner normalizes every candidate test name by stripping a
leading `test_` prefix and lowercasing the remainder. It then looks
up the normalized form in the committed test corpus
(`tests/**/*.rs` integration tests plus
`src/**/*.rs` inline `#[test]`-annotated functions). Any match is a
violation.

Candidate test names in plan prose come from two sources:

1. **Rust declarations** inside fenced code blocks:
   `fn <snake_name>(` lines.
2. **Backtick-quoted identifiers** in prose: tokens matching
   `^[a-z_][a-z0-9_]{9,}$` (length ≥ 10 characters) inside
   backticks. The length filter prevents common-word identifiers
   like `foo_bar` from false-positive matching.

The matching is symmetric: a plan naming `test_foo_bar_quux_blocks`
and an existing `foo_bar_quux_blocks` both normalize to
`foo_bar_quux_blocks` and collide.

## Opt-Out Grammar

Two line-level HTML comments suppress a trigger:

- `<!-- duplicate-test-coverage: not-a-new-test -->` — the plan
  prose is discussing or referencing an existing test by name, not
  proposing a new one. Use when a plan's Exploration section cites
  an existing test as prior art.
- `<!-- duplicate-test-coverage: intentional-duplicate -->` — the
  author is knowingly adding a parallel test whose name collides
  with an existing test. See the "Named Tests After Refactor"
  section of `.claude/rules/docs-with-behavior.md` for the class of
  case this handles: a refactor that makes a test appear redundant
  still requires the named test to exist, driven through a test
  seam so the caller-level contract is independently asserted.

Walk-back grammar matches sibling rules: the comment applies to
its own line, the next non-blank line, or two lines below with a
single blank line between. No chaining across more than one blank
line. This mirrors `.claude/rules/scope-enumeration.md` and
`.claude/rules/external-input-audit-gate.md`.

## Enforcement Topology

Three callsites share `duplicate_test_coverage::scan`:

- **Standard plan path** — `bin/flow plan-check`
  (`src/plan_check.rs::run_impl`), invoked from
  `skills/flow-plan/SKILL.md` Step 4 before `phase-transition
  --action complete`.
- **Pre-decomposed extracted path** — `src/plan_extract.rs`
  extracted path, runs the scanner against the promoted plan
  content for issues filed via `/flow:flow-create-issue`.
- **Resume path** — `src/plan_extract.rs` resume path, re-runs the
  scanner against the existing plan file when the user re-enters
  Phase 2 after a prior violation.

All three callsites return the same JSON error shape
(`status="error"`, `violations[]`, `message`) with per-violation
`rule="duplicate-test-coverage"` tags, so the repair loop is
identical regardless of which path triggered the failure.

A corpus contract test in `tests/duplicate_test_coverage.rs`
covers the committed prose surfaces (`CLAUDE.md`,
`.claude/rules/*.md`, `skills/**/SKILL.md`,
`.claude/skills/**/SKILL.md`) so future regressions in those
surfaces fail CI immediately.

## How to Apply

When `bin/flow plan-check` returns a violation tagged
`duplicate-test-coverage`:

1. Read the cited line in the plan file. The violation's
   `existing_test` and `existing_file` fields name the pre-existing
   test and its location.
2. Decide the correct path:
   - **Rename** — if the new test exercises a distinct property,
     rename it so the normalized form differs from the existing
     test's normalized form.
   - **Strengthen** — if the new test exercises the same property,
     delete the new-test proposal from the plan and strengthen the
     existing test if its assertion is weaker than the plan
     envisioned.
   - **Opt-out** — if the duplication is intentional per the
     "Named Tests After Refactor" pattern, add the
     `<!-- duplicate-test-coverage: intentional-duplicate -->`
     comment near the trigger line with a brief justification.
3. Re-run `bin/flow plan-check`. If clean, proceed to phase
   completion.

If the trigger is a prose discussion of an existing test rather
than a new-test proposal, add the
`<!-- duplicate-test-coverage: not-a-new-test -->` opt-out using
the walk-back rule above.

## Vocabulary Extensibility

The length-filter threshold (≥ 10 characters) and the two-item
opt-out grammar are closed and curated. Novel false-positive
phrasings are handled by extending the vocabulary in follow-up
commits, mirroring the discipline documented in
`.claude/rules/scope-enumeration.md` "Vocabulary Extensibility."
The rule file is the primary instrument; the scanner is the
merge-conflict trip-wire.

## Cross-References

- `.claude/rules/tests-guard-real-regressions.md` — the prose
  discipline this gate enforces mechanically.
- `.claude/rules/scope-enumeration.md` — structurally sibling
  gate; shares the opt-out grammar and three-callsite topology.
- `.claude/rules/external-input-audit-gate.md` — the other
  structurally sibling gate.
- `.claude/rules/docs-with-behavior.md` "Named Tests After
  Refactor" — the motivating class for the
  `intentional-duplicate` opt-out.
- `src/duplicate_test_coverage.rs` — the scanner implementation.
- `src/plan_check.rs` — the standard-path gate.
- `src/plan_extract.rs` — the extracted and resume gates.
- `tests/duplicate_test_coverage.rs` — the corpus contract test.
