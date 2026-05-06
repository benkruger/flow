# Testing Gotchas

## Fixture Safety

Never create symlinks to real binaries in test fixtures.
Writing to a symlink follows it and overwrites the target.
Use wrapper scripts (`exec <real_path> "$@"`) instead of symlinks
when tests need a fake executable at a known path.

## Host Environment Leaks

When a test calls code that internally runs `current_branch()`,
`project_root()`, or any git subprocess without setting the working
directory to the fixture repo, the subprocess resolves against the
host repo. Tests that pass on feature branches but fail on main are
the symptom — the host branch name accidentally matched (or didn't
match) the fixture branch name. Always set `current_dir` to the
fixture repo in tests that pass `branch=None` to functions with
auto-detect fallbacks.

Trace every fixture operation that touches real system resources.
When a test fixture creates references to real files, binaries, or
executables, mentally trace every subsequent operation. If any
operation could follow a reference back to the real resource and
mutate it, the fixture is unsafe. Replace indirect references with
self-contained fakes that cannot escape the temp directory.

## Rust Parallel Test Env Var Races

Rust's test runner executes tests in parallel by default. Never use
`unsafe { std::env::set_var() }` or `std::env::remove_var()` inside
Rust tests — concurrent tests that read the same env var will race
and produce intermittent failures. A test that passes in isolation
(`cargo test single_test_name`) but fails in the full suite is the
symptom. The fix is to extract a pure function that accepts the
values as parameters (e.g. `build_config(token: &str, channel: &str)`)
and test that function directly. The env-var-reading wrapper
(`read_config() -> build_config(env::var(TOKEN), env::var(CHANNEL))`)
is kept for production but not exercised by unit tests.

## Test Failure After Change: Question the Change First

When a test fails after your change, the first question is "is my
change wrong?" — not "should I update the test?" Adjusting a test
to accommodate a change that is itself the bug produces a green CI
that hides the real problem. Only update the test after confirming
the change is correct.

## Distinguish Environmental Load From Flaky Tests

When a test fails or times out, the first question is whether the
machine is under unusual external load (video call, screen
recording, large parallel build, OS update, antivirus scan, etc.).
Test timing under `cargo-llvm-cov` instrumentation is sensitive to
CPU contention; subprocess-spawning tests are sensitive to fork/exec
lock contention. Heavy single-machine load can push a healthy test
past a tight `slow-timeout` boundary in `.config/nextest.toml`.
That is environmental noise — not a defect, not a regression, not a
flaky test.

**Never file a "Flaky Test" issue based on a single failure during
known-heavy machine load, period.** No exceptions. The right
response is to wait for load to return to normal and re-run the
test. A test that fails consistently across multiple runs under
**different** load conditions is genuinely flaky; one that fails
under documented heavy load and passes when isolated is
environmental.

**Never modify shared infrastructure (`.config/nextest.toml`,
`.config/*`, test-group overrides, parallelism limits) to paper
over single-machine load events.** That expands the PR's diff into
shared territory the user has not approved and conflates an
environmental incident with a real config change.

**Investigation steps before classifying as flaky:**

1. Re-run the test in isolation
   (`bin/flow ci --test -- <test_name>`). If it passes within a
   second of expected runtime, the failure was environmental.
2. Re-run the full CI suite when the machine is idle (no video
   calls, no parallel builds, no other heavy workloads). If it
   passes, the failure was environmental.
3. Only after multiple consecutive failures across different load
   conditions — say, three back-to-back failures spanning two
   different days or load profiles — does the test merit a "flaky
   test" classification.

## Cross-Branch Verification Before Claiming Infrastructure Bugs

Before concluding that a build tool, test runner, or CI script
itself has a bug, run the same command on the integration branch
to establish a baseline. A failure or anomalous timing observation
on a worktree is evidence of something — but until both observations
exist, the failure cannot be located in the worktree's changes
versus the shared infrastructure those changes inherit.

The default attribution when a test command misbehaves is "my
branch broke it," not "the tool is broken." The worktree contains
uncommitted changes; the integration branch contains code that has
already passed CI. If the same command passes on the integration
branch and fails or behaves unexpectedly in the worktree, the
worktree is the cause. If the command reproduces the same behavior
on both, only then is "infrastructure bug" a credible diagnosis.

**The Rule.** Before declaring `bin/test`, `bin/flow ci`,
`cargo`, `nextest`, or any other build/test tool buggy, slow, or
broken:

1. Capture the failing or anomalous command and its observed
   timing/exit code in the worktree.
2. Run the exact same command on the integration branch. Capture
   timing and exit code there.
3. Compare. Only when the integration-branch run reproduces the
   anomaly does "tool bug" become a valid hypothesis. Otherwise
   the worktree's changes are the cause — investigate them.

A diagnosis without step 2 is speculation. The user's evidence —
"it works on main" — is ground truth per the
"User evidence is ground truth" convention in CLAUDE.md.

**When this triggers.** Any time the model is about to claim
"`bin/test` is slow," "the runner has a bug," "CI is broken,"
"the suite timeout is wrong," or any equivalent statement that
locates the fault in shared infrastructure rather than in the
current branch's changes.

## Ambiguous Check Name Filters

When filtering a list of check results by name substring, verify the
substring is unique across all check names. A broad substring like
`"completed"` can match multiple checks (e.g. "Two or more flows
completed" and "All flows completed all phases"), causing assertions
to validate the wrong check. Use the most specific distinguishing
substring.

## Subprocess-Repopulated Directories

When testing a subprocess that cleans files in a directory, and that
same subprocess later runs code which repopulates the directory,
assert on specific target files rather than directory existence.
Asserting `not dir.exists()` will fail because the subprocess
recreated the directory after the cleanup step; asserting
`not stale_file.exists()` correctly verifies the cleanup happened
because the recreated directory contains only fresh files.

## Test Doc Comment Must Support the Test Name

A test's doc comment should describe what the test verifies in terms
consistent with the test function's name. Never write a doc comment
that disavows the test name's assertion — e.g., a test named
`deps_stdout_does_not_corrupt_return_value` whose comment says "the
structural guarantee lives in production, not this test". If the
test's exercise path only indirectly verifies the property the name
claims (e.g., the property is enforced structurally in production
code, and the test trip-wires a regression of that structure), the
comment should explain HOW the exercise path trip-wires a regression
of the named property.

## Message Content Assertions — Per Variant, Not Just Presence

When a function returns a human-readable message that names a specific
command, path, or identifier, and the function handles multiple
variants of that input (e.g. `bin/flow ci` and `bin/ci`), every test
that exercises a different variant must assert on the message content,
not just `msg.is_some()`. A single hardcoded message string that names
only one variant will silently mislead callers who triggered the
function via the other variant.

Pattern:

```rust
#[test]
fn test_bin_ci_variant_produces_correct_message() {
    let msg = should_block_background("bin/ci", false);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("bin/ci")); // content, not just presence
}
```

How to apply: when writing tests for a function with multi-variant
message output, enumerate the variants in the test list and add one
content assertion per variant.

## Suffix-Match Path Coverage

When a function uses `ends_with("/path/segment")` for matching a
file or binary (e.g. `first.ends_with("/bin/ci")`), tests must
include BOTH the bare form (`bin/ci`) and the absolute-path form
(`/Users/name/project/bin/ci` or `/opt/tools/bin/ci`). Parallel
tests for each path variant document the intended coverage and
catch bugs where the suffix match is silently broken.

```rust
#[test]
fn test_bare_form_matches() {
    assert!(is_flow_command("bin/ci"));
}

#[test]
fn test_absolute_path_matches() {
    assert!(is_flow_command("/Users/me/project/bin/ci"));
}
```

How to apply: during Plan phase, enumerate every `ends_with` pattern
the implementation will use, then add one test per pattern for each
form (bare + absolute).

## Subsection-Local Assertions in Contract Tests

When a contract test asserts that a file contains specific content
inside a named section — a Markdown heading, a Rust `mod` block, a
YAML sub-document — bound the assertion's search scope to the
section itself, not the entire file. The failure mode is silent: a
test that splits on a heading and checks `contains()` over the
remainder will be satisfied by unrelated content elsewhere in the
file, so a refactor that guts the section passes CI as long as any
sibling section still carries the expected substring.

### Why

`split("H").nth(1)` returns the *entire* remainder of the file from
the first occurrence of `"H"` forward. Any later section in the
same file that happens to mention the asserted token satisfies the
assertion — including unrelated sibling sections.

If the test's English claim is "the X subsection routes through Y,"
the slice must cover only that subsection, not everything after its
opening heading.

### The pattern

Walk the slice to the section start, then walk it to the next
section boundary:

```rust
// CORRECT — subsection covers only the content between the
// heading and the next level-3 heading
let tail_at_heading = c
    .split_once("### Measurement-Only Tasks")
    .map(|(_, tail)| tail)
    .expect("heading checked above");
let subsection = tail_at_heading
    .split_once("\n### ")
    .map(|(section, _)| section)
    .unwrap_or(tail_at_heading);
assert!(subsection.contains("/flow:flow-commit"));
```

`split_once` is preferred over `split().nth(1)` because it makes
the intent explicit (one split, two pieces).

For Markdown files, "next section boundary" is usually the next
heading of the same or higher level. The end delimiter should
match the heading marker of the section being tested:

- For a `### ` subsection, split on `"\n### "`.
- For a `## ` section, split on `"\n## "`.

For Rust source files, use the `fn ` or `mod ` tokens that bound
the unit under test. For YAML, use the top-level key that bounds
the sub-document.

### Fallback to EOF

When the section being tested is the last section in the file, use
`.unwrap_or(tail)` so the assertion scope falls back to the end of
the file rather than panicking.

### How to apply

When writing a new contract test that asserts content inside a
named section:

1. Identify the heading or boundary marker that starts the section.
2. Identify the marker that ends the section.
3. Walk to the start using `split_once(start_marker)`.
4. Walk to the end using a second `split_once(end_marker)` on the
   tail, falling back to the tail itself via `unwrap_or(tail)`.
5. Run all content assertions against the bounded `subsection`
   slice, never against the full file content.

When reviewing an existing contract test that uses
`split(marker).nth(n)` or a raw `contains()` over the full file,
grep the file being tested for the asserted substrings. If any of
them appear in multiple sections, the test is fragile — replace it
with the bounded-slice pattern above.

## macOS Subprocess Path Canonicalization

When a subprocess test spawns a child binary with `current_dir(dir)`
and the child's production code computes paths from its `current_dir()`,
the test fixture's path construction must match the child's view of
the cwd — not the parent's. On macOS, `tempfile::tempdir()` returns a
path under `/var/folders/...`, which is a symlink to
`/private/var/folders/...`. The child's `std::env::current_dir()`
resolves through the symlink and returns the canonical
`/private/var/` form. If the test then constructs a `file_path` from
the non-canonical `dir.path()` and passes it to the child, any
production `starts_with` prefix check between the child's canonical
cwd-derived project_root and the test's non-canonical file_path
silently fails — and whichever fallback the production code takes
(often an "outside project = allow" early return) fires instead of
the branch the test claims to verify. The test passes vacuously.

**The rule.** Every subprocess-spawning test that computes a file
path for the child's `tool_input` (or equivalent payload) must
canonicalize the tempdir root before constructing any descendant
path. Do this once at the top of the test body and carry the
canonical `root` through every `join()` call:

```rust
#[test]
fn my_subprocess_test() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();  // <-- canonical
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = root.join("src/lib.rs");
    // ... spawn child with current_dir(&worktree) ...
}
```

**Why allow-path tests need this too.** The temptation is to think
"only the block path compares paths, so only block tests need
canonicalization." That is wrong. Allow paths also take their
classification based on path comparisons — the "outside project"
early-return is itself a code path, and a test that passes the
"outside project" branch when it expected to test the ".flow-states
allow" branch is vacuous. The fix is universal: canonicalize
everywhere.

**How to apply.** When reviewing a new subprocess test that spawns a
child binary and passes a file_path constructed from the tempdir
root, check that the test either canonicalizes at construction
time or spawns with a cwd that shares the same
canonicalization state as the file_path. Tests that fail this check
are vacuous on macOS — fix them by canonicalizing.

## Document Test Fixture Helpers

Test fixture helpers that create worktrees, state files, settings
files, or similar test environments are part of the test
infrastructure — not scratch code. Every fixture helper that other
tests depend on must have a doc comment that explains:

1. What the helper returns (including what filesystem state it
   creates as a side effect)
2. What each parameter controls and what values mean (especially
   for boolean flags like `with_state_file: bool` and slice
   parameters like `allow_patterns: &[&str]`)
3. Any production invariants the helper must satisfy that are
   non-obvious (e.g., writing a `.git` marker file so
   `detect_branch_from_cwd` succeeds instead of falling back to
   `git branch --show-current`)

A newcomer adding a test to the same file must be able to discover
the helper's contract without reading its body or tracing the
production code it emulates. The reference pattern is
`setup_worktree_fixture` and `setup_pretool_fixture` in
`tests/hooks.rs`, whose doc comments call out the `.git` marker
rationale, the `with_state_file` branch, and the `allow_patterns`
format.

## Timing-Sensitive Test Isolation

When a test verifies behavior that depends on elapsed time
(staleness, timeout, expiration), never use `thread::sleep` or
real-time delays. Inject a time-control seam so the test sets the
clock without waiting.

**Why.** Real-time delays introduce flakiness — a test that sleeps
100ms and expects a stale entry to be cleaned up fails when system
load delays the check past the threshold. Sleep-based tests also
slow the suite: ten 100ms sleeps add a full second of wall-clock
time per CI run, compounding across the test corpus.

**Patterns by domain:**

- **Filesystem mtime** — use `filetime::set_file_mtime` to
  backdate or forward-date entries. Reference: the `start_lock`
  tests in `tests/start_lock.rs` use `FileTime` to simulate stale
  queue entries without sleeping.
- **Wall-clock functions** — accept an injectable `now_fn`
  closure parameter so tests can return a controlled timestamp.
- **Retry/timeout loops** — accept an injectable `sleep_fn`
  closure so the test can count calls without blocking. Reference:
  `acquire_with_wait_impl` in `src/commands/start_lock.rs` injects
  `sleep_fn: F` where tests pass a no-op or a side-effecting
  closure.

**How to apply.** When writing a test for time-dependent behavior,
identify which time source the production code reads (mtime, system
clock, sleep duration) and inject a seam at that point. If the
production code does not accept a seam, refactor it to accept one
before writing the test.
