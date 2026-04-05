# Testing Gotchas

## Function Alias Tautology

When converting a subprocess test to in-process and the converted test
compares two function calls (`result == other_module.f(same_args)`),
check first whether both names refer to the same object (`f is g`).
If they are the same, the comparison is tautological — replace with
behavioral assertions (`isinstance`, content checks, specific values).

## Fixture Safety

Never create symlinks to real binaries in test fixtures.
`Path.write_text()` follows symlinks and overwrites the target.
Use wrapper scripts (`exec <real_path> "$@"`) instead of symlinks
when tests need a fake executable at a known path.

## Host Environment Leaks

When a test calls code that internally runs `current_branch()`,
`project_root()`, or any git subprocess without `monkeypatch.chdir`
to the fixture repo, the subprocess resolves against the host repo.
Tests that pass on feature branches but fail on main are the
symptom — the host branch name accidentally matched (or didn't
match) the fixture branch name. Always use `monkeypatch.chdir(git_repo)`
in tests that pass `branch=None` to functions with auto-detect
fallbacks.

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

## Coverage Changes After Main Merge

When CI coverage reports unexpected statement counts (e.g. a file
shows 456 stmts instead of 398), check whether main was recently
merged into the branch before diagnosing cross-process interference.
Merging main brings in new source lines from other PRs — this is
the most common cause of coverage data "changing" between CI runs.
Do not attribute coverage changes to env var leaking or subprocess
interference without first verifying the file contents are unchanged.

## Test Failure After Change: Question the Change First

When a test fails after your change, the first question is "is my
change wrong?" — not "should I update the test?" Adjusting a test
to accommodate a change that is itself the bug produces a green CI
that hides the real problem. Only update the test after confirming
the change is correct.

## Ambiguous Check Name Filters

When filtering a list of check results by name substring (e.g.
`[c for c in checks if "completed" in c["name"]]`), verify the
substring is unique across all check names. A broad substring like
`"completed"` can match multiple checks (e.g. "Two or more flows
completed" and "All flows completed all phases"), causing assertions
to validate the wrong check. Use the most specific distinguishing
substring.

## Mocked Subprocess Blind Spots

When tests mock `subprocess.run` for an external CLI tool (`gh`,
`curl`, `git`), the mock accepts any arguments — it cannot catch
invalid flags, unknown fields, or misspelled subcommands. If the
code constructs CLI arguments dynamically (e.g. `--json` field
lists), verify the field names are valid against the tool's actual
interface, not just against what the mock returns. Passing tests
with mocked subprocesses do not prove the command works.

## JSON Null vs Absent Keys

When parsing external API JSON (GitHub GraphQL, REST, Slack),
`dict.get("key", {})` returns `None` — not `{}` — when the key
exists with value `null`. Use `data.get("key") or {}` to handle
both absent and null cases. This applies to every chained `.get()`
call on external API responses where fields can be null.

When converting JSON values with `int()`, `float()`, or `str()`,
remember that `int(None)` raises `TypeError` — not `ValueError`.
Always include `TypeError` in except clauses that parse JSON fields
which could be `null`. This applies to any JSON file read from
disk, not just API responses.

## Pre-Normalized Test Values

When a function compares a parameter against a transformed value
(e.g. `path.stem`, `branch_name()` output, lowercased strings),
tests that pass already-normalized values as both the parameter and
the fixture data cannot detect transformation mismatches at the call
site. Include at least one test with realistic unsanitized input
(spaces, capitals, special characters) to verify the caller applies
the same normalization the comparison expects.

## Subprocess-Repopulated Directories

When testing a subprocess that cleans files in a directory, and that
same subprocess later runs code which repopulates the directory
(e.g. `bin/ci` cleans `tests/__pycache__/` then runs pytest which
recreates it during collection), assert on specific target files
rather than directory existence. Asserting `not dir.exists()` will
fail because the subprocess recreated the directory after the
cleanup step; asserting `not stale_file.exists()` correctly verifies
the cleanup happened because the recreated directory contains only
fresh files.

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
    assert!(is_ci_command("bin/ci"));
}

#[test]
fn test_absolute_path_matches() {
    assert!(is_ci_command("/Users/me/project/bin/ci"));
}
```

How to apply: during Plan phase, enumerate every `ends_with` pattern
the implementation will use, then add one test per pattern for each
form (bare + absolute). The test count is small — two tests per
pattern — and it locks the intended match surface.
