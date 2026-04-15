# Test Coverage Waivers

Lines in the flow-rs Rust tree that are architecturally unreachable
from tests. Each entry names specific file:line coordinates and a
one-sentence reason. New waivers require a matching plan task —
do not add entries without a recorded architectural justification.

## src/analyze_issues.rs

After the `run_with_drain_and_timeout` / `gh_result_to_stdout`
extraction (PR #1153), the remaining uncovered lines fall into two
architectural categories.

### run() gh-issue-list subprocess block (lines 528–557)

These lines execute only when `analyze-issues` is invoked WITHOUT
`--issues-json`, meaning the CLI spawns a real `gh issue list`
subprocess. The test suite cannot exercise this path without
network I/O and authenticated GitHub access, which would introduce
flakiness and make CI depend on external services.

- `src/analyze_issues.rs:529–538` — `gh_args` vector construction
  for the `gh issue list` invocation. Only reached when the CLI
  runs without `--issues-json`.
- `src/analyze_issues.rs:539–546` — `--label` and `--milestone`
  flag propagation into `gh_args`. Same reachability.
- `src/analyze_issues.rs:547–549` — `Command::new("gh")` + argument
  binding + helper invocation. Delegates to the fully-tested
  `run_with_drain_and_timeout` helper.
- `src/analyze_issues.rs:551–557` — `gh_result_to_stdout` dispatch
  + `std::process::exit(1)` on error. `gh_result_to_stdout` is
  unit-tested independently; the `std::process::exit(1)` line
  cannot be reached from inside the calling process.

The subprocess behavior itself is exercised by
`run_with_drain_and_timeout` tests (`helper_success_returns_stdout`,
`helper_nonzero_exit_returns_stderr`,
`helper_timeout_kills_child_and_returns_timedout_error`,
`helper_spawn_error_is_surfaced`, `helper_large_stdout_does_not_deadlock`)
and the result-interpretation logic by the `gh_result_to_stdout`
tests (`gh_result_to_stdout_success_returns_stdout`,
`gh_result_to_stdout_nonzero_exit_returns_labeled_error`,
`gh_result_to_stdout_timeout_returns_timeout_error`,
`gh_result_to_stdout_spawn_error_returns_labeled_error`,
`gh_result_to_stdout_uses_command_label`), both via synthetic
`sh -c` commands with no dependency on `gh`.

### analyze_issues chrono alternate-format fallback (lines 415–423)

- `src/analyze_issues.rs:415–423` — defensive RFC 3339 retry path
  for createdAt strings that fail the primary
  `chrono::DateTime::parse_from_rfc3339`. In practice, GitHub's
  API returns ISO 8601 strings that the primary parser accepts
  natively (`Z` is valid RFC 3339), so the fallback branch is
  unreachable for well-formed API output. The branch exists as
  a graceful-degradation safety net against future GitHub API
  format changes; removing it would introduce a panic surface
  we do not want.

### run() `std::process::exit(1)` lines (external only)

- `src/analyze_issues.rs:524` — `std::process::exit(1)` after
  `--issues-json` read failure. External observation via
  subprocess exit code covered by `cli_missing_file`.
- `src/analyze_issues.rs:564` — `std::process::exit(1)` after
  JSON parse failure. External observation via subprocess exit
  code covered by `cli_malformed_json`.
- `src/analyze_issues.rs:607` — `std::process::exit(1)` after
  filter error. Unreachable because `filter_name` is selected
  from a closed set (`"ready"`, `"blocked"`, `"decomposed"`,
  `"quick-start"`) by the `run()` function itself; every
  member of that set is a valid `filter_issues` filter name.
  The `Err(e)` arm exists as a defensive guard against future
  filter-set drift.

All other lines in `src/analyze_issues.rs` are covered by the
inline unit test module and the integration CLI tests that spawn
the compiled binary with `--issues-json` fixtures.

## src/complete_fast.rs

After the `run_impl_inner` extraction (issue #1137), `run_impl` is a
3-line wrapper threading production dependencies into the testable
inner function, and the CI dirty-check body lives in
`production_ci_decider`. The remaining uncovered regions fall into
two architectural categories: the `run()` CLI entry (terminates the
test process via `process::exit`) and the `production_ci_decider`
paths that delegate to `ci::run_impl` (require a real CI
subprocess).

### `run()` CLI wrapper (lines 607-620)

`run()` invokes `run_impl(&args)` and routes the result: on `Ok`
with `status == "error"` it prints and calls `std::process::exit(1)`;
on `Err` it prints the error JSON and calls `std::process::exit(1)`.
Both exit calls terminate the calling test process, so the exit arms
cannot be reached from inside a Rust `#[test]`. The testable surface
is `run_impl` / `run_impl_inner`, both covered by the inline test
module.

- `src/complete_fast.rs:609-614` — `Ok` branch with `println!` of
  the result and the error-status exit arm. Reached indirectly via
  any integration test that drives the CLI subcommand; the exit call
  cannot be reached from inside a Rust `#[test]`. Standard
  CLI-entry pattern per `.claude/rules/rust-patterns.md` (CLI
  Testability — run_impl Pattern).
- `src/complete_fast.rs:615-618` — `Err` branch. Same pattern; the
  testable surface is `run_impl_inner` which returns
  `Result<Value, String>` and is exercised by the
  `test_run_impl_inner_*` cases.

### `production_ci_decider` real-CI delegation (lines 408-450)

`production_ci_decider` contains the former inline CI dirty-check
body from pre-refactor `run_impl` (issue #1137). Its branches split
into testable structure and untestable delegation:

- `src/complete_fast.rs:414-416` — `tree_changed=true` early return.
  Covered by `production_ci_decider_tree_changed_returns_not_skipped`.
- `src/complete_fast.rs:418-427` — `tree_changed=false` sentinel
  lookup and snapshot comparison. Requires a live `cwd` with a real
  `tree_snapshot` and a sentinel file whose contents match that
  snapshot. Achievable only from an integration test that runs in a
  prepared git tree — unit-test fixtures using `tempfile::tempdir()`
  cannot produce matching `tree_snapshot` output because
  `tree_snapshot` reads HEAD, diff, and untracked files via git
  subprocess.
- `src/complete_fast.rs:429-449` — CI invocation path. Calls
  `ci::run_impl(&ci_args, cwd, root, false)` which spawns the
  full `bin/format` / `bin/lint` / `bin/build` / `bin/test` chain in
  `cwd`. Unit tests cannot inject this path without running real CI
  on the host system; the test seam `run_impl_inner(args, root,
  runner, ci_decider)` exists specifically to bypass this callsite
  in unit tests by supplying a mock closure. The branches inside
  this arm (zero vs non-zero `ci_code`, `message` field lookup) are
  exercised by the `run_impl_inner` tests that pass
  `ci_failed_decider` and `no_ci` mock closures — those closures
  return the same two outputs this production arm produces.

The testable surface — `run_impl_inner` plus its ten `test_run_impl_inner_*`
cases — covers every dispatch branch downstream of this decider.
The decider itself is intentionally thin (a glue layer over
`ci::tree_snapshot`, `ci::sentinel_path`, and `ci::run_impl`, each
of which has its own test coverage in `src/ci.rs`).

### Note: `fetch_blockers` error-path coverage

The plan (PR #1153, Task 5) originally listed named tests for
`fetch_blockers` error branches (`fetch_blockers_returns_empty_on_spawn_failure`,
`fetch_blockers_returns_empty_on_timeout`,
`fetch_blockers_returns_empty_on_nonzero_exit`). Those named tests
were not added as stand-alone cases because `fetch_blockers` now
delegates its subprocess discipline to `run_with_drain_and_timeout`
and its error-formatting to `gh_result_to_stdout` — both of which
have full branch coverage via inline unit tests that exercise each
failure mode with synthetic `sh -c` commands (see the
`// --- run_with_drain_and_timeout ---` and `// --- gh_result_to_stdout ---`
section markers). Combined with the `eprintln!` observability path
added in this PR, every error branch `fetch_blockers` can take is
exercised by existing tests; adding dedicated `fetch_blockers_*`
variants would be duplicate coverage.

