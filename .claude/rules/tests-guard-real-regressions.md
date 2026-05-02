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

### Multi-file contract tests

A contract test that asserts content invariants across more than
one file (e.g. a skill change requiring synchronized updates to
four agent `## Input` sections) is valid under this rule **only
when each file's assertion guards a distinct regression**. Each
file's regression must be named individually; "the agents were
updated together" is not a named regression — the test must
explain what specifically breaks if any one of the files drifts.

**Default shape: per-file siblings.** Rather than a single
`multi_file_contract` test asserting "all four agents reference
the new diff range," write four separate tests — one per file —
each naming the agent and the regression
(`reviewer_agent_input_describes_substantive_diff`,
`pre_mortem_agent_input_describes_substantive_diff`, etc.).
Per-file tests give failure output that names the drifted file
immediately, instead of forcing the maintainer to read assertion
internals to find which file regressed.

**Single-test shape: only when coordination is the invariant.**
When the assertion is genuinely a single coordinated invariant
across files (e.g. "all four agents reference the same diff
range string, and the string must match the skill's invocation"),
a single multi-file test is acceptable. Its doc comment MUST
state:

1. Why splitting the assertions would lose the coordination
   property the test guards.
2. What regression each file's branch of the assertion guards
   individually (one short sentence per file).
3. The canonical example file a maintainer should read first
   when the test fails (so failure triage starts at the right
   place).

A multi-file test that lists "many files updated together"
without per-file regression statements is speculation — split
into per-file siblings or strengthen the doc comment per the
single-test shape requirements.

**Plan-phase requirement.** When a plan task proposes a contract
test that spans multiple files, the plan's Tasks section must
state which shape applies (per-file siblings vs. single
coordinated test) and, if single, must include the three
doc-comment items above as part of the task description. Code
Review's reviewer agent flags multi-file tests that arrive
without this discipline as Real findings.

### Corpus-scan viability check

When a PR proposes a new corpus contract test (scanning the
committed prose corpus for a rule's forbidden pattern), run a
viability check **before** writing the test. The check is already
documented in `.claude/rules/scope-enumeration.md` "False-positive
sweep before expanding the vocabulary" for one specific case
(vocabulary expansion), but it applies universally to any
corpus-class contract test:

1. **Run the scanner over the current corpus.** Apply the candidate
   trigger vocabulary to `CLAUDE.md`, `.claude/rules/*.md`,
   `skills/**/SKILL.md`, and `.claude/skills/**/SKILL.md` and count
   how many lines the scanner would flag.
2. **Classify the flags.** If the count is **zero or low (≤ 4)**,
   audit each flagged line — genuine missing items are fixed in the
   same PR, false triggers get opt-out comments. If the count is
   **high (≥ 5)**, the scanner has a false-positive problem
   intrinsic to the project's existing prose.
3. **On high false-positive count, defer the corpus test.** The
   candidate is not viable as a mechanical enforcer in this
   codebase. Ship only the Plan-phase gate (which scans plan
   content, not the committed prose corpus) and document the
   deferral in the rule file's Enforcement section with the
   false-positive count and the legitimate-citation examples that
   triggered it.
4. **Replace the contract test with a documented marker.** Leave
   `tests/<scanner-name>.rs` as an intentionally empty integration
   test file whose module doc comment records the decision. A
   future session looking for the contract test finds the rationale
   without re-deriving it.

**Motivating incident.** PR #1177's `duplicate_test_coverage`
scanner flagged 18+ legitimate educational test-name citations in
the prose corpus (e.g. `test_agent_frontmatter_only_supported_keys`
in CLAUDE.md documenting an enforcement mechanism,
`production_ci_decider_tree_changed_returns_not_skipped` in
`extract-helper-refactor.md` as a reference pattern). The Plan-phase
gate catches the real regression path (a plan proposing a duplicate
test name is flagged at plan-check time regardless of where the
name appeared in committed prose), so the corpus scanner added no
protection on top of the existing gate. `tests/duplicate_test_coverage.rs`
now ships as a documented empty marker per step 4 above.

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

## Coverage-Required Tests

The 100% coverage gate (`--fail-under-lines/regions/functions` in
`bin/test`, backed by `.claude/rules/no-waivers.md`) makes every
production line a named consumer. A test whose sole purpose is to
cover a branch that has no other named consumer is NOT speculation
under this rule — it satisfies the rule by naming:

- **The specific regression.** A future edit deletes this line
  without a test witness, or a refactor makes the line unreachable
  without a test that would notice.
- **The code path.** `cargo-llvm-cov nextest` reports the line
  uncovered on the next `bin/flow ci` run, which trips the
  `--fail-under-*` gate in `bin/test` and blocks the commit.
- **The named consumer.** The 100% coverage gate itself — the
  `--fail-under-*` flags in `bin/test` and the
  `.claude/rules/no-waivers.md` discipline that forbids ever
  lowering the thresholds.

Coverage-required tests should be tightly scoped: one test per
branch, asserting what the branch produces or returns. Avoid
exercising adjacent branches in the same test body; one test per
branch keeps the regression path unambiguous when the test fails.

### Placement

Every test lives in `tests/<name>.rs` parallel to `src/<name>.rs`
and drives through the public interface per
`.claude/rules/test-placement.md`. Coverage-required tests follow
the same placement rule — no inline `#[cfg(test)]` blocks in
`src/*.rs`.

Two execution modes within `tests/<name>.rs` cover the breadth of
the coverage surface:

- **Library-level tests** — call `pub` items from the `flow_rs`
  crate directly (`run_impl_main` seams, public helpers, injected
  closure variants). Used when the branch under test is reachable
  through the public surface of the subject module. Reference
  pattern: `tests/complete_fast.rs` exercises
  `run_impl_with_deps` through the public seam.
- **Subprocess tests** — spawn the compiled binary via
  `CARGO_BIN_EXE_flow-rs` to exercise CLI dispatch, real
  filesystem interactions, or behavior that requires a fresh
  process. Reference pattern: subprocess tests in
  `tests/main_dispatch.rs` and `tests/concurrency.rs`.

If a coverage-required branch resists both modes, the fix is one
of the three default responses in `.claude/rules/no-waivers.md`:
add a subprocess test, refactor the code to make the branch
testable through the public surface (seam injection), or delete
the branch as unreachable dead code. Never make a private item
`pub` solely to enable the test.

Group related coverage-required tests under a section-marker
comment naming the branch family the tests cover (see
`.claude/rules/rust-patterns.md` "Test Module Section Markers").
Naming conventions follow the production function's name so a
grep from code to test is immediate.

### Mutation-style verification

When reviewing a coverage-required test, verify it trips when the
covered line is deleted. The three-step procedure:

1. Run `bin/flow ci --test -- <test_name>` and confirm the test passes
   against the current implementation.
2. Comment out (or delete) the production line the test claims to
   cover. Re-run `bin/flow ci --test -- <test_name>` and confirm the
   test now fails.
3. Restore the production line. Re-run once more and confirm the
   test passes again.

A test that still passes with the line removed is speculative —
it is not actually exercising the line. Strengthen the assertion
(check a value the line produces, not just that the function
returns without panic) before committing.

## Frozen-Golden Tests

Some tests pin the exact output of a deterministic computation
(SHA-256 prefix, JSON canonicalization, snapshot text, hash of
constant inputs) so a future refactor cannot silently change the
output's bytes. The frozen-golden test:

- Calls the production function and asserts equality against a
  hardcoded literal value
- Treats any difference as a regression, including changes that
  appear "intentional" — the author of the change must update the
  golden value AND understand the downstream impact

Frozen-golden tests guard a real regression path: any change to
the function's algorithm, formatter, key order, or input
constants invalidates downstream consumers (e.g., stored hash
values in user config files, persisted snapshots, cross-version
checksums). The named consumer is the persisted artifact whose
correctness depends on byte-stability.

### Bootstrapping the golden value safely

Discovering the golden value by running the production code and
copying its output into the test is the fastest path but
provides ZERO regression protection if the code is wrong at
authoring time. The test would assert the buggy output equals
itself.

The discipline:

1. **Verify the value independently before pinning.** Compute the
   expected output through a separate path — a reference
   implementation, a spec-derived calculation, manual computation
   on a small input, or cross-check against an existing
   downstream artifact (e.g., a stored `.flow.json` hash from a
   previous release). Document the verification path in the
   test's doc comment.
2. **If no independent path exists, pin the value but require a
   second-source confirmation.** Add an inline comment naming the
   environment, dependency versions, and reference inputs used to
   compute the golden value, so a future maintainer trying to
   reproduce can verify.
3. **Document the update protocol.** The test's doc comment must
   explain: when intentionally changing the function's output
   (algorithm, format, inputs), the author updates the golden
   value in the same commit and notes the migration impact in the
   commit message.

### Placement

A frozen-golden test lives alongside the function it tests in
`tests/<name>.rs`. Group with other tests for the same function
under a section marker. Tag the golden constant with
`CURRENT_<purpose>` (e.g., `CURRENT_CONFIG_HASH`) so a grep for
the prefix surfaces every frozen value in the codebase.

A reference implementation: `compute_config_hash_uses_python_default_formatter`
in `tests/prime_check.rs` pins a 12-character SHA-256 prefix
produced from `UNIVERSAL_ALLOW`/`FLOW_DENY`/`EXCLUDE_ENTRIES`
through the `PythonDefaultFormatter`. The pinned value protects
every stored `.flow.json` hash in the wild from silent
invalidation.

## How to Apply

**Plan phase.** When a plan task adds a test, the task description
must include a one-line statement of (1), (2), and (3). A test
task that cannot state them is incomplete — revise the task or
drop it. For plan tasks that propose a corpus contract test, the
plan must also state whether the viability check has been run and
what the false-positive count was.

**Code phase.** Before writing a test, state (1), (2), and (3)
internally. If you are about to write "This test guards against
future drift" or "This test ensures no regressions," stop — name
the specific regression or delete the test. For corpus contract
tests, run the viability check as the first action; if the count
is ≥ 5, defer the test and document the deferral in the rule file.

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
- `.claude/rules/scope-enumeration.md` "False-positive sweep before
  expanding the vocabulary" — the original protocol that the
  Corpus-scan viability check section generalizes.
- `.claude/rules/skill-authoring.md` "Plan Task Ordering" — TDD
  discipline that this rule complements.
- `.claude/rules/duplicate-test-coverage.md` "Enforcement Topology"
  — a rule that intentionally ships without a corpus contract
  test per the viability check above, with the deferral rationale
  inline.
- `.claude/rules/no-waivers.md` — the 100% coverage discipline
  that names coverage-required tests as having a valid named
  consumer.
- `.claude/rules/rust-patterns.md` "Test Module Section Markers"
  — naming and grouping conventions for coverage-required tests
  placed inline in source files or grouped in `tests/*.rs`.
