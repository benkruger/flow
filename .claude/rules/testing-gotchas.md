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
