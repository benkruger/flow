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
