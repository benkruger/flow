# Tests Guard Real Regression Paths

Every test must guard a real regression path with a named consumer.
Before adding a test, name the specific regression it guards and the
code path that produces that regression. If neither exists, the test
is speculation, not verification.

## Why

Tests earn their place in the suite by preventing specific bugs. A
test added "for safety" without a concrete regression to prevent
bloats the suite without catching anything that would have shipped
broken. Speculative tests have three costs:

1. They run on every CI invocation forever.
2. They invite expansion — "while we're here, let's also scan for
   X" — and never contract.
3. They mislead future readers into believing the property they
   assert is actively at risk, when in fact no code path produces
   the risk today.

The project already has strong mechanical enforcement for the
drift surfaces that matter: tombstones in `tests/tombstones.rs`,
corpus scanners like `tests/scope_enumeration.rs` and
`tests/external_input_audit.rs` (each with a named trigger
vocabulary backed by a concrete incident), and plan-check gates.
Adding broader "safety net" scans on top of that accumulates test
code without covering new regressions.

## The Rule

When adding any test — unit test, integration test, contract test,
corpus scan, tombstone — state the following before writing it:

1. **The specific regression.** What exact change to the code, prose,
   or configuration would break the property this test asserts?
2. **The code path that produces the regression.** What mechanism
   — a merge conflict, a refactor, an accidental edit, a missing
   cross-reference — would cause that change to land?
3. **The named consumer.** What rule, skill, hook, or other test
   relies on the property being true? Name it.

If any of (1), (2), or (3) cannot be named, the test is
speculation. Delete it, or rewrite it to guard a regression you can
name.

### Three valid test shapes under this rule

- **Tombstones** — guard a specific named deletion. The regression
  is a merge-conflict resurrection; the consumer is the fact that
  the deleted content is gone. See
  `.claude/rules/tombstone-tests.md`.
- **Structural contract tests** — assert a specific invariant in a
  specific file (e.g., "flow-plan SKILL.md contains the
  Extract-Helper Branch Enumeration subsection"). The regression
  is an accidental edit; the consumer is the skill's cross-
  reference or the subsection's role in the workflow.
- **Targeted corpus scans** — the scanner must have a named
  trigger vocabulary tied to a documented incident and a named
  consumer (the rule file that authorizes the scan). See
  `tests/scope_enumeration.rs` and
  `tests/external_input_audit.rs`. Broader scans without a named
  incident or vocabulary are speculative.

### Forbidden patterns

- **"Just in case"** scans over broad file sets without a named
  regression path.
- **"For future drift"** tests where the drift mechanism is
  unspecified.
- **Duplicate guards** for a property already covered by an
  existing tombstone, plan-check scanner, or structural contract
  test.
- **Corpus-wide scans for a forbidden substring** when the
  substring's only known occurrences are in files that must
  legitimately discuss the forbidden term (requiring an ever-
  growing exemption list).

## How to Apply

**Plan phase.** When a plan task adds a test, the task description
must include a one-line statement of (1), (2), and (3). A test
task that cannot state them is incomplete — revise the task or
drop it.

**Code phase.** Before writing a test, state (1), (2), and (3)
internally. If you are about to write "This test guards against
future drift" or "This test ensures no regressions," stop — name
the specific regression or delete the test.

**Code Review phase.** The reviewer agent treats any test that
cannot be traced to a named regression as a Real finding. The fix
is either tightening the test to a specific invariant or
deleting it.

**Learn phase.** User corrections that flag speculative tests
surface as missing-rule findings. This rule is the reference.

## Motivating Incident

Issue #1160 / PR #1168 surfaced this. During the Code phase, I
added a `no_waiver_language_in_authoring_corpus` contract test
that scanned `.claude/rules/*.md`, `CLAUDE.md`, `skills/**/SKILL.md`,
and `.claude/skills/**/SKILL.md` for forbidden waiver substrings,
with an exemption list for `no-waivers.md` and the new
`extract-helper-refactor.md`. The test was ~100 lines of Rust. The
user flagged it: main already has three specific tombstones
covering the three surfaces where waivers had been historically
introduced (`test_coverage.md`, `docs-with-behavior.md` Waiver
Discipline section, CLAUDE.md `test_coverage.md` references), plus
the `.claude/rules/no-waivers.md` rule prose, plus plan-check
scanners. The corpus scan's only realistic regression paths were
already covered; the scan would only fire on the exempt files
themselves (silently). I reverted the test.

## Cross-References

- `.claude/rules/tombstone-tests.md` — the canonical form for
  guarding named deletions.
- `.claude/rules/scope-enumeration.md` and
  `.claude/rules/external-input-audit-gate.md` — canonical forms
  for targeted corpus scans with named trigger vocabularies.
- `.claude/rules/skill-authoring.md` "Plan Task Ordering" — TDD
  discipline that this rule complements.
