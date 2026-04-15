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
consistent with the test function's name. Never rewrite a doc comment
during code review in a way that disavows the test name's assertion —
e.g., a test named `deps_stdout_does_not_corrupt_return_value` whose
comment says "the structural guarantee lives in production, not this
test". If the test's exercise path only indirectly verifies the
property the name claims (e.g., the property is enforced structurally
in production code, and the test trip-wires a regression of that
structure), the comment should explain HOW the exercise path
trip-wires a regression of the named property — not that the property
is someone else's responsibility. A reader whose first exposure to
the test is its name should find the comment affirmatively supporting
the name, not contradicting it.

## Message Content Assertions — Per Variant, Not Just Presence

When a function returns a human-readable message that names a specific
command, path, or identifier, and the function handles multiple
variants of that input (e.g. `bin/flow ci` and `bin/ci`), every test
that exercises a different variant must assert on the message content,
not just `msg.is_some()`. A single hardcoded message string that names
only one variant will silently mislead callers who triggered the
function via the other variant — the test passes because the message
exists, but the content is wrong.

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
content assertion per variant. If the function returns the same
message for every variant, use a generalized message that names all
variants so the assertion is meaningful across the test set.

## Suffix-Match Path Coverage

When a function uses `ends_with("/path/segment")` for matching a
file or binary (e.g. `first.ends_with("/bin/ci")`), tests must
include BOTH the bare form (`bin/ci`) and the absolute-path form
(`/Users/name/project/bin/ci` or `/opt/tools/bin/ci`). Parallel
tests for each path variant document the intended coverage and
catch bugs where the suffix match is silently broken (e.g. a
refactor that accidentally changes `ends_with` to `starts_with`).

Pattern for every `ends_with(path)` callsite in production code:

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
form (bare + absolute). The test count is small — two tests per
pattern — and it locks the intended match surface.

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

When a new section is added to a multi-section file (for example,
a subsection inside `skills/flow-code/SKILL.md` whose job is to
route a specific task shape to `/flow:flow-commit`), a contract
test proves the subsection exists and carries the correct routing.
A naive implementation looks like:

```rust
// WRONG — after_heading covers everything from the heading to EOF
let after_heading = c
    .split("Measurement-Only Tasks")
    .nth(1)
    .expect("heading checked above");
assert!(after_heading.contains("/flow:flow-commit"));
```

`split("H").nth(1)` returns the *entire* remainder of the file from
the first occurrence of `"H"` forward. Any later section in the
same file that happens to mention `/flow:flow-commit` satisfies the
assertion — including the standard Commit section that every
iteration of the skill has always had. A malicious (or merely
careless) refactor that empties the new subsection of its
`/flow:flow-commit` reference while leaving the rest of the file
intact passes CI because the later unrelated mention still lives in
`after_heading`.

The same class of gap appears whenever the assertion scope exceeds
the logical unit under test. If the test's English claim is "the
Measurement-Only Tasks subsection routes through `/flow:flow-commit`,"
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
the intent explicit (one split, two pieces) and avoids the iterator
`nth()` ambiguity on strings that contain multiple occurrences of
the split delimiter.

For Markdown files, "next section boundary" is usually the next
heading of the same or higher level. The end delimiter should
match the heading marker of the section being tested:

- For a `### ` subsection, split on `"\n### "` (stops at the next
  `### ` or a higher-level `## `/`# ` by virtue of the newline
  anchor and the assumption that the subsection's parent ends with
  `## `, not `### `).
- For a `## ` section, split on `"\n## "`.

For Rust source files, use the `fn ` or `mod ` tokens that bound
the unit under test. For YAML, use the top-level key that bounds
the sub-document.

### Fallback to EOF

When the section being tested is the last section in the file, the
next-section split returns no matches. Use `.unwrap_or(tail)` so
the assertion scope falls back to the end of the file rather than
panicking. This keeps the test robust against a future edit that
reorders sections and leaves the one under test at EOF.

### How to apply

When writing a new contract test that asserts content inside a
named section:

1. Identify the heading or boundary marker that starts the section.
2. Identify the marker that ends the section (the next peer heading,
   the next mod block, the next top-level key).
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

The motivating incident is benkruger/flow#1167 — the initial
contract test for the Measurement-Only Tasks subsection matched
`/flow:flow-commit` anywhere after the heading, including the
standard Commit section ~L443 of `skills/flow-code/SKILL.md`. A
gutted subsection passed the test. The fix bounded the slice with
`split_once("### Measurement-Only Tasks")` followed by
`split_once("\n### ")`. This rule codifies the pattern so future
contract tests ship bounded from the start.

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
