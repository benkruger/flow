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

## The Rule

The scanner normalizes every candidate test name via
`normalize(name)` — lowercase first, then strip a leading `test_`
prefix — so matching is symmetric across case and prefix. It then
looks up the normalized form in the committed test corpus
(`tests/**/*.rs`). Any match is a violation. Inline `#[cfg(test)]`
blocks in `src/*.rs` are prohibited by
`.claude/rules/test-placement.md`, so the corpus scope is
`tests/**/*.rs` only.

The corpus regex recognizes `#[test]` attributes followed by a
function declaration regardless of:

- Intervening attributes (`#[ignore]`,
  `#[should_panic(expected = "...")]`, `#[cfg(feature = "...")]`,
  `#[cfg_attr(...)]`).
- Modifiers (`pub`, `pub(crate)`, `async`, `unsafe`, `const`,
  `extern "C"`).
- Same-line declarations (`#[test] fn foo() {}`) vs. newline-
  separated forms.

Candidate test names in plan prose come from two sources:

1. **Rust declarations** inside fenced code blocks (both backtick
   ` ``` ` and tilde `~~~` fences per CommonMark):
   `fn <snake_name>(` lines.
2. **Backtick-quoted identifiers** in prose: tokens matching
   `(?i)^[a-z_][a-z0-9_]{9,}$` (length ≥ 10 characters,
   case-insensitive) inside backticks. Captured content is trimmed
   before the length/shape check so padded backticks like
   `` ` foo_bar_baz_quux ` `` do not silently bypass the scanner.
   The length filter prevents common-word identifiers like
   `foo_bar` from false-positive matching.

The matching is symmetric: a plan naming `test_foo_bar_quux_blocks`
and an existing `foo_bar_quux_blocks` both normalize to
`foo_bar_quux_blocks` and collide. Case is also symmetric —
`TEST_FOO_BAR_QUUX_BLOCKS` normalizes identically.

## Opt-Out Grammar

Two line-level HTML comments suppress a trigger:

- `<!-- duplicate-test-coverage: not-a-new-test -->` — the plan
  prose is discussing or referencing an existing test by name, not
  proposing a new one.
- `<!-- duplicate-test-coverage: intentional-duplicate -->` — the
  author is knowingly adding a parallel test whose name collides
  with an existing test. See the "Named Tests After Refactor"
  section of `.claude/rules/docs-with-behavior.md` for the class of
  case this handles.

Walk-back grammar matches sibling rules: the comment applies to
its own line, the next non-blank line, or two lines below with a
single blank line between. No chaining across more than one blank
line.

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
`rule="duplicate-test-coverage"` tags.

### No corpus contract test

Unlike the sibling `scope_enumeration` and `external_input_audit`
rules, this rule intentionally ships without a corpus contract
test over `CLAUDE.md`, `.claude/rules/*.md`, `skills/**/SKILL.md`,
and `.claude/skills/**/SKILL.md`. Legitimate educational citations
of test names in rule files would false-positive at scale.

Per `.claude/rules/tests-guard-real-regressions.md`, the corpus
scan adds no protection on top of the Plan-phase gate already
shipped — a plan that copy-pastes a test name from committed prose
is caught by the same `plan_check` invocation that runs over plan
content. `tests/duplicate_test_coverage.rs` ships as a documented
empty marker; its module doc comment records the rationale.

## How to Apply

When `bin/flow plan-check` returns a violation tagged
`duplicate-test-coverage`:

1. Read the cited line in the plan file. The violation's
   `existing_test` and `existing_file` fields name the pre-existing
   test and its location.
2. Decide the correct path:
   - **Rename** — if the new test exercises a distinct property,
     rename it so the normalized form differs.
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

## Pre-Audit at Plan Authoring Time

The mechanical scanner catches duplicates after the plan is
already written, but a plan author writing test names by hand
should pre-audit before naming the test — particularly when the
plan is filed via `/flow:flow-create-issue` from an issue body
the author drafted under one mental model of the codebase.

When a plan task proposes writing tests in a file path that
already exists, the Exploration section MUST list the existing
test functions in that file before the Tasks section names new
tests. The Plan author cross-checks every proposed test name
against the enumerated existing list and either renames or
deletes any collision before submitting the plan.

The cheapest signal: in Exploration, when a "Files in scope"
entry points at a `tests/` path, the author runs Glob+Read on
that path and notes the test count and the section markers.
A plan that lists "Files: tests/foo.rs" without acknowledging
that foo.rs already contains 50+ tests is missing this audit
step — the gate will catch the duplicates at phase-transition
time but at the cost of a full plan-rewrite cycle.

The pattern recurs most often with pre-decomposed issues
(`/flow:flow-create-issue`) because the issue body is drafted
ahead of plan-time codebase exploration. Plan authors fed a
pre-decomposed issue with a `## Implementation Plan` section
must NOT trust the test names in the issue verbatim — they must
re-validate against the current state of the test corpus before
finalizing the plan.

## Vocabulary Extensibility

The length-filter threshold (≥ 10 characters) and the two-item
opt-out grammar are closed and curated. Novel false-positive
phrasings are handled by extending the vocabulary in follow-up
commits, mirroring the discipline documented in
`.claude/rules/scope-enumeration.md`.
