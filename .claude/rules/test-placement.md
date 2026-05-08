# Test Placement

Every test lives at `tests/<path>/<name>.rs`, mirroring
`src/<path>/<name>.rs`, and drives the subject code through its
public interface. No inline `#[cfg(test)]` blocks or test-only
items in `src/*.rs`.

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
boundary.

Three secondary wins fall out of the placement rule:

- **Mirrored paths** — `tests/<path>/<name>.rs` matches
  `src/<path>/<name>.rs` by pure string substitution. No lookups,
  no ambiguity. Every src file with tests has exactly one mirror
  in `tests/` at the same relative path.
- **Fast per-file green loop** — `bin/test tests/<path>/<name>.rs`
  drives the mirrored src file to 100/100/100 in isolation. Edit
  the src file, run the one test file, see red or green.
- **Clean diffs** — production edits and test edits land in
  separate files. A source-file diff shows only behavior change; a
  test-file diff shows only coverage change.

## The Rule

- Every test lives under `tests/`. No `#[cfg(test)]` attributes or
  blocks appear in `src/**/*.rs`.
- `tests/` mirrors `src/` structurally. The test file for
  `src/<path>/<name>.rs` is `tests/<path>/<name>.rs` — pure
  `s/^src/tests/` substitution, no lookups. Examples:
  - `src/tui.rs` → `tests/tui.rs`
  - `src/commands/set_timestamp.rs` →
    `tests/commands/set_timestamp.rs`
  - `src/hooks/stop_continue.rs` → `tests/hooks/stop_continue.rs`
- Cargo registers test binaries via auto-discovery — the
  `Cargo.toml` manifest never carries `[[test]]` stanzas for any
  test in this codebase. Two layouts cover every case:
  - **Top-level tests** (`tests/<name>.rs`) — Cargo's default
    integration-test glob discovers them automatically; one binary
    per file, named after the file basename.
  - **Subdirectory tests** (`tests/<subdir>/<name>.rs`) — Cargo's
    directory form discovers ONE binary per directory rooted at
    `tests/<subdir>/main.rs`. Sibling files become modules of that
    binary ONLY when declared via `mod <name>;` in `main.rs`.
    The binary is named after the directory (e.g. `tests/hooks/`
    produces a `hooks` binary; sibling `tests/hooks/<name>.rs`
    files become `hooks::<name>` modules).
- `Cargo.toml` is package and dependency configuration, not test
  registration. `bin/dependencies` is the only sanctioned editor;
  routine test-adding work must never touch it. Per
  `.claude/rules/permissions.md` "Shared Config Files," every
  engineer-visible edit to `Cargo.toml` requires explicit user
  permission.
- Shared test helpers live in `tests/common/mod.rs`. Top-level
  tests import them via `mod common;` at the file top.
  Subdirectory tests delegate the path-aliased declaration to the
  directory's `main.rs` (one `#[path = "../common/mod.rs"] mod
  common;` line in `main.rs`); each sibling module references
  helpers as `crate::common::<name>` rather than redeclaring the
  module locally.
- Tests drive the subject through `pub` items exposed by the crate
  (library `pub` functions, `pub` types, `run_impl_main` seams, the
  compiled binary via `CARGO_BIN_EXE_flow-rs`). A test that cannot
  be written against the public surface is a signal that the public
  surface is incomplete — add the seam, do not expose the private
  helper.

### Subdirectory `main.rs` shape

A canonical subdirectory entry point looks like:

```rust
//! Cargo's directory-form auto-discover layout for `tests/<subdir>/`.

#[path = "../common/mod.rs"]
mod common;

mod first_sibling;
mod second_sibling;
mod third_sibling;

#[allow(dead_code)]
fn main() {}
```

The `#[allow(dead_code)] fn main() {}` body satisfies Rust's
binary-target requirement; the `--test` harness replaces it at test
build time with the auto-generated test runner that collects every
`#[test]` function reachable from the declared modules.

### Meta-tests without a src counterpart

A small number of tests have no matching src file because they
assert project-wide conventions rather than a single module's
behavior. These live at `tests/` root as explicit exceptions to
the mirror rule:

- `tests/test_placement.rs` — this contract test
- `tests/tombstones.rs` — consolidated tombstone assertions
- `tests/skill_contracts.rs` — SKILL.md content contracts
- `tests/structural.rs` — config invariants
- `tests/permissions.rs` — permission allow/deny simulation
- `tests/docs_sync.rs` — docs completeness
- `tests/opt_out_inventory.rs` — frozen list of bypass comments in the rule corpus

Adding a new meta-test without a src counterpart requires amending
this list. Every other test under `tests/` must mirror a src file.

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
design — this is a drift tripwire, not a negotiation surface.

When a src file genuinely needs the characters `#[cfg(test)]` in a
string literal (e.g., a test-corpus scanner's fixture construction),
there is exactly one canonical escape: split the literal via
`concat!("#[cfg", "(test)]")`. The `concat!` output produces the
same runtime string without placing the literal substring on any
source line.

No other escapes exist. If a src file is flagged:

1. If the line contains an actual `#[cfg(test)]` attribute or block,
   move the test to `tests/<path>/<name>.rs` (mirroring the src
   file) per this rule.
2. If the line contains the substring inside a string literal,
   rewrite using `concat!` as above.
3. If the line contains the substring inside a block comment,
   rewrite the comment using `//` line comments (project
   convention) or drop the substring.

## How to Apply

**New code.** Write the test file at the mirrored path under
`tests/` first. If the subject src file doesn't exist yet, its
public surface is what the test exercises — design the public API
from the test side. For a subdirectory mirror, ensure the
directory's `main.rs` declares the new file as a `mod`; never add a
`[[test]]` stanza to `Cargo.toml`.

**Migrating inline tests out of src.** Open `src/<path>/<name>.rs`
and create its mirror at `tests/<path>/<name>.rs` (or open the
existing mirror if one exists). Move every `#[cfg(test)] mod
tests` block from the src file to the tests file. For each moved
test:

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
   the public surface without a production consumer.

   **Bright-line test for `pub` additions.** Before adding `pub`
   to any item, the author MUST name the non-test production
   consumer outside this module, in the commit message and in the
   item's own doc comment. If the only callers are (a) a thin
   production wrapper (`run()`, a main.rs match arm, or any other
   same-module dispatcher that exists to forward to this item) and
   (b) integration tests, the `pub` is for testing and is
   forbidden. No exceptions for mirroring pre-existing anti-patterns
   elsewhere in the codebase — pre-existing exposures that fail
   this test are debt to be audited, not precedent to copy.

   **Forbidden naming shapes as pub.** The following function-name
   suffixes/shapes are overwhelmingly indicative of pub-for-testing
   unless accompanied by a named non-test cross-module consumer:
   `_inner`, `_impl`, `_with_runner`, `_with_resolver`,
   `_with_deps`, `_with_tty`, `_with_timeout`, and any
   `run_impl_main_with_*` variant that exists alongside a real
   `run_impl_main`. When one of these shapes is proposed as `pub`,
   the author must either (a) provide the named consumer or
   (b) keep the item private and drive tests through the real
   production entry point via subprocess or fixture.

   **Carve-out: externally-coupled test seams.** The
   `rust-patterns.md` "Seam-injection variant for externally-coupled
   code" section defines ONE legitimate class of `pub` test seam:
   variants that inject dependencies `cargo nextest` genuinely
   cannot supply — real TTY, raw-mode terminal, network socket,
   live crossterm event loop. This carve-out is closed. CI
   runners, git subprocess output, gh subprocess output, state
   file reads, sentinel file state, tree snapshots, and PR number
   parsing are NOT externally coupled in this sense — they are
   fixture-controllable from integration tests. A `pub` seam for
   any of those fails the bright-line test above.

   **When the test resists the real production path.** If a branch
   cannot be reached through the real production entry, the fix is
   one of (in priority order):

   1. `.expect("<rationale>")` on the unreachable arm per
      `.claude/rules/testability-means-simplicity.md`. The
      `.expect` does not count against coverage because it does
      not create a branch.
   2. Delete the branch entirely if it has no production
      consumer — unreachable defensive code is a code smell.
   3. Restructure the function so the hard-to-test branch is
      gone (simpler primitive, fewer seams). `Command::output()`
      instead of hand-rolled timeout loops. `From<io::Error>`
      instead of `.map_err(|e| e.to_string())`.
   4. Only then — and only then — introduce a `pub` seam, with a
      named non-test consumer documented in the doc comment.

   A `pub` addition whose justification is "the test needs it" is
   forbidden no matter how the wording is dressed up. The
   justification must name a real caller in real production code.
4. If the new test file lives under a `tests/` subdirectory, declare
   it as a `mod <name>;` line in the directory's `main.rs`. Never
   add `[[test]]` stanzas to `Cargo.toml` — the directory-form
   layout handles registration. Reference shared helpers as
   `crate::common::<name>` (the path-aliased common module is
   declared once in `main.rs`).
5. Run `bin/test tests/<path>/<name>.rs` and iterate until the
   mirrored src file reads 100/100/100.

**Relocating a legacy flat test file.** When a legacy test lives
at `tests/<name>.rs` but mirrors a src file under a subdirectory
(e.g., `tests/set_timestamp.rs` mirrors
`src/commands/set_timestamp.rs`), move it to the mirrored path:

1. `git mv tests/<name>.rs tests/<path>/<name>.rs`.
2. Add a `mod <name>;` line to `tests/<path>/main.rs` so the
   directory's binary picks the file up. Never add a `[[test]]`
   stanza.
3. Drop the file's local `mod common;` declaration (the
   directory's `main.rs` declares it path-aliased) and rewrite
   `common::*` references to `crate::common::*`.
4. Verify `bin/test tests/<path>/<name>.rs` still runs the tests.

**Migrating section markers.** The `// --- <primary_name> ---`
grouping convention from `.claude/rules/rust-patterns.md` still
applies inside the integration test file — the home of the markers
moves from `src/<path>/<name>.rs` to `tests/<path>/<name>.rs`.
