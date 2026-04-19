# Test Placement

Every test lives in `tests/<name>.rs`, parallel to `src/<name>.rs`,
and drives the subject code through its public interface. No inline
`#[cfg(test)]` blocks or test-only items in `src/*.rs`.

## Why

Interface-only testing forces the public surface to be complete
enough to exercise every branch. When a branch can only be reached
via a private helper, one of two things is true:

1. The branch has no legitimate public path and should be deleted
   (see `.claude/rules/testability-means-simplicity.md`).
2. The public surface is missing a seam (injectable dependency,
   fallible constructor variant, closure parameter) that the caller
   — and the test — need alike. Add the seam.

Both outcomes improve the code. Neither requires moving the privacy
boundary. Calling code reaches private helpers through a public
entry point every time; tests do the same.

Three secondary wins fall out of the placement rule:

- **Parallel naming** — `tests/foo.rs` matches `src/foo.rs` (or
  `src/commands/foo.rs`, `src/hooks/foo.rs`) so the test surface for
  any source file is discoverable by name alone. Every src file with
  tests has exactly one mirror in `tests/`.
- **Fast per-file green loop** — `bin/test tests/<name>.rs` drives
  the parallel src file to 100/100/100 in isolation. Edit the src
  file, run the one test file, see red or green. No need to reason
  about which inline `mod tests` block covers which branch.
- **Clean diffs** — production edits and test edits land in
  separate files. A source-file diff shows only behavior change; a
  test-file diff shows only coverage change. Review is easier to
  scope and the Code phase's commit boundaries align with the
  atomic-commit rule.

## The Rule

- Every test lives under `tests/`. No `#[cfg(test)]` attributes or
  blocks appear in `src/**/*.rs`.
- The test file for `src/<path>/<name>.rs` is `tests/<name>.rs`. The
  `tests/` directory is flat — subpaths in `src/` collapse to a bare
  filename in `tests/` (e.g. `src/commands/set_timestamp.rs` →
  `tests/set_timestamp.rs`, `src/hooks/stop_continue.rs` →
  `tests/stop_continue.rs`).
- Tests drive the subject through `pub` items exposed by the crate
  (library `pub` functions, `pub` types, `run_impl_main` seams, the
  compiled binary via `CARGO_BIN_EXE_flow-rs`). A test that cannot
  be written against the public surface is a signal that the public
  surface is incomplete — add the seam, do not expose the private
  helper.
- Test helpers and fixtures used across test files live in
  `tests/common/mod.rs` or in a dedicated helper module declared
  within a `tests/` file. They never live in `src/`.

### Test-only items in `src/`

Test-only `use` statements, helper functions, and types gated by
`#[cfg(test)]` are prohibited in `src/`. If a helper is needed only
by a test, it lives with the test. If the helper is needed by both
production and tests, it is production code (no `#[cfg(test)]`
gate) and its public form serves both callers.

## Enforcement

`tests/test_placement.rs::src_contains_no_inline_cfg_test_blocks`
walks every `.rs` file under `src/` and flags any line that
contains the literal `#[cfg(test)]` outside a `//` line comment.
Flagged contexts include:

- Real attributes (`#[cfg(test)] mod tests { ... }`) — the primary
  target.
- Block comments (`/* #[cfg(test)] */`).
- Raw string literals (`r#"#[cfg(test)]"#`) and normal string
  literals (`"#[cfg(test)]"`).
- Any other surface that produces the exact substring on a line.

A single flagged line fails the build. The scanner is strict by
design — this is a drift tripwire, not a negotiation surface. It
is not lowered, opt-outed, or relaxed to handle edge cases.

When a src file genuinely needs the characters `#[cfg(test)]` in a
string literal (e.g., a test-corpus scanner's fixture construction),
there is exactly one canonical escape: split the literal via
`concat!("#[cfg", "(test)]")`. The `concat!` output produces the
same runtime string without placing the literal substring on any
source line. This matches the existing `concat!("#[", "ignore",
"]")` pattern used in `tests/duplicate_test_coverage.rs` fixtures
to avoid tripping the `no_skipped_or_excluded` contract test.

No other escapes exist. If a src file is flagged:

1. If the line contains an actual `#[cfg(test)]` attribute or block,
   move the test to `tests/<name>.rs` per this rule.
2. If the line contains the substring inside a string literal,
   rewrite using `concat!` as above.
3. If the line contains the substring inside a block comment,
   rewrite the comment using `//` line comments (project
   convention) or drop the substring.

## How to Apply

**New code.** Write the test file under `tests/<name>.rs` first. If
the subject src file doesn't exist yet, its public surface is what
the test exercises — design the public API from the test side.

**Migrating an existing file.** Open `src/<path>/<name>.rs` and its
peer `tests/<name>.rs` (create it if absent). Move every
`#[cfg(test)] mod tests` block from the src file to the tests file.
For each moved test:

1. Replace `use super::*;` with `use flow_rs::<module>::*;` (or a
   more specific path through the library crate) — the test now
   imports from the public surface.
2. If a test references a private helper by name, follow one of:
   - Drive the test through the public entry point that already
     calls the helper. This is almost always possible and is the
     preferred fix.
   - Extract the needed behavior into an injectable seam on the
     public surface (closure parameter, `run_impl_with_deps`-style
     variant) and test the seam. See
     `.claude/rules/rust-patterns.md` "Seam-injection variant for
     externally-coupled code."
   - If neither works, the branch under test is a signal of
     over-engineering per
     `.claude/rules/testability-means-simplicity.md`. Simplify
     the src file until every branch reaches the public surface.
3. Never make a private item `pub` solely to enable the test.
   That inverts the rule's intent — exposure for testing expands
   the public surface without a production consumer, and every
   future maintainer must treat the exposed item as part of the
   crate's contract.
4. Run `bin/test tests/<name>.rs` and iterate until the parallel
   src file reads 100/100/100.

**Migrating section markers.** The `// --- <primary_name> ---`
grouping convention from `.claude/rules/rust-patterns.md` still
applies inside the integration test file — the home of the markers
moves from `src/<name>.rs` to `tests/<name>.rs`, the convention
itself is unchanged.

## Cross-References

- `.claude/rules/testability-means-simplicity.md` — the principle
  this rule mechanically enforces. When a branch can't be tested
  via the public surface, simplify or seam-inject; never widen
  privacy.
- `.claude/rules/tests-guard-real-regressions.md` — every test in
  the migrated tests file must still guard a named regression, one
  per branch.
- `.claude/rules/rust-patterns.md` — seam-injection patterns
  (`run_impl_main`, `run_impl_with_deps`, closure-parameter
  variants) that make interface-only testing tractable for
  externally-coupled code.
- `.claude/rules/no-waivers.md` — 100/100/100 coverage gate.
  Interface-only testing must still reach the gate; when a branch
  resists public-surface testing, the answer is one of the three
  responses in that rule (subprocess test, refactor, design
  change), never a waiver.
- `tests/test_placement.rs` — the contract test that enforces the
  rule at CI time.
