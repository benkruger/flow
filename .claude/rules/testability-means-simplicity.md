# Testability Means Simplicity

Code is tested for three reasons:

1. **Prove it works** — the tests demonstrate the happy path and the
   error paths produce the outputs the spec requires.
2. **Detect over-engineering** — if a branch is hard to cover,
   the code is more complex than the problem requires. The
   un-testable shape is the bug, not the test gap.
3. **Regression** — future edits cannot silently undo the
   behavior the test locks in.

Reason **(2)** is the diagnostic. When a coverage gap resists
straightforward tests — requires mock traits, subprocess-only
paths, per-codegen-unit monomorphization hunts, or elaborate
seam injection — **stop writing tests and simplify the code
instead**.

## The diagnostic in practice

Signals that the code is over-engineered:

- A function needs a new trait + mock struct just to drive one
  error branch.
- A branch's Err region is covered in one binary instance but
  invisible to another — the `?` propagation is splitting
  reachability across compilation units that can't all exercise
  it with reasonable tests.
- The test needs a subprocess with a fake `$PATH`, a non-
  executable binary, or a signal-terminated child to hit a
  single region.
- The test fixture is longer than the function under test.
- The function exists only because another function needed a
  seam to be testable.
- You find yourself adding `#[inline(always)]` or `#[cfg(test)]`
  to eliminate a monomorphization gap.

Any of these means the function is doing too much or papering
over a too-complex control flow. The fix is to reduce the
function's surface, not to add more test scaffolding.

## How to apply

When a coverage gap resists two or three straightforward tests:

1. **Stop and describe the function in one sentence.** If the
   sentence contains "and" or "with", the function does more
   than one thing.
2. **Identify the single purpose.** What does the calling code
   actually need?
3. **Pick the simplest standard library primitive that meets
   that need.** (`Command::output()` instead of hand-rolled
   child drain threads; `std::fs::read_to_string()` instead of
   a custom reader; a `match` ladder instead of a trait seam
   for three cases.)
4. **Delete the infrastructure that existed only to make the
   over-engineered version testable** — the trait, the mocks,
   the seam-injection variants, the `_with_runner` / `_with_deps`
   helpers. If their only caller is tests, they shouldn't exist.
5. **Re-write the tests against the simpler function.** They
   should now be boring: call the function, assert the output.

## Example

`run_with_drain_and_timeout` in `src/analyze_issues.rs` was
40 lines that spawned a `Child`, drained stdout/stderr in
background threads, polled `try_wait()` in a loop, and killed
on timeout. To make it testable required a `ChildController`
trait, a `MockChild` struct, a seam-injected
`wait_for_exit_with_timeout` helper, and explicit unit tests
for each trait method — all of it existing only because the
real `Child` type couldn't be mocked.

The single sentence: "call `gh`, return stdout or an error."

The simple primitive: `std::process::Command::output()`. It
drains automatically, it returns a `Result`, and every branch
is a single `match` arm any test can drive. No timeout (gh has
its own network timeout; a hung process is a Ctrl-C scenario,
not a coverage concern).

Result: the 40-line function collapses to ~15, the trait +
mock + seam helper delete entirely, and the two stubborn
uncovered-region gaps disappear because the regions no longer
exist.

## Cross-references

- `.claude/rules/tests-guard-real-regressions.md` — every test
  must name a specific regression it guards. Coverage-required
  tests that exist only to hit an over-engineered branch are
  not naming a real regression.
- `.claude/rules/rust-patterns.md` — seam-injection variant
  patterns (`run_impl_with_deps`, `ChildController`) are
  legitimate when the production caller genuinely needs a
  dependency it cannot mock in-process (TTYs, real sockets, &c.).
  They become over-engineering when the simpler primitive would
  have sufficed.
- `.claude/rules/no-waivers.md` — the 100% coverage gate forces
  this discipline. When you can't reach 100% on a branch, the
  rule says fix the code, not the threshold.
