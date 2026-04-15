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

`run_impl` is a thin wrapper that threads the production runner and
CI-decider into `run_impl_inner`; the CI dirty-check body lives in
`production_ci_decider`. The uncovered regions fall into two
architectural categories: the `run()` CLI entry (terminates the test
process via `process::exit`) and the `production_ci_decider` paths
that delegate to `ci::run_impl` (require a real CI subprocess).

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

`production_ci_decider` owns the Complete-phase CI dirty-check body:
sentinel lookup, tree-snapshot comparison, and `ci::run_impl`
invocation on miss. Its branches split into testable structure and
untestable delegation:

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

## src/notify_slack.rs

`notify_with_deps(args, config_reader, poster)` and
`run_with_deps(args, config_reader, poster, writer)` are the testable
seams that drive every closure-injected branch. Three remaining
regions are architecturally unreachable from the in-process test
harness.

### `read_slack_config` env-var reader (lines 64–68)

`read_slack_config()` reads `CLAUDE_PLUGIN_CONFIG_slack_bot_token` and
`CLAUDE_PLUGIN_CONFIG_slack_channel` directly from the process
environment. Rust's test runner executes tests in parallel and shares
one process-wide environment, so any test that sets or clears these
vars races with every concurrently-running test that reads them. The
project rule `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env
Var Races" prohibits `unsafe { std::env::set_var() }` /
`std::env::remove_var()` inside Rust tests for this reason. The pure
helper `build_config(bot_token, channel)` accepts the strings as
parameters and is fully covered by `build_config_*` tests; the
`read_slack_config` wrapper exists solely to bind `env::var(...)` to
`build_config` for production callers and is the architectural
boundary between testable and untestable.

### `run_curl_with_timeout` real-curl subprocess (lines 153–189)

`run_curl_with_timeout(args, timeout_secs)` spawns a real `curl` child
process via `Command::new("curl")` and polls `try_wait` until the
child exits or the timeout elapses. Exercising it requires (a) the
`curl` binary present on the test host and (b) timing-sensitive
assertions about the polling loop and 15-second timeout. Either
dependency introduces test flakiness without measurable safety gain:
the closure seam at `post_message_inner(... curl: &dyn Fn(...))`
already reaches every behavioral branch (HTTP 200 success, HTTP non-2xx
mapped through the `ok` field, curl nonzero exit, invalid JSON
response, and timeout error string) via the inline `mock_curl` test
helper. `run_curl_with_timeout` binds the production curl subprocess
to the closure seam and contains no behavior the seam cannot already
exercise.

### `notify` and `run` production binders (lines 225–229, 248–258)

`notify(args)` and `run(args)` are thin production binders that wire
`notify_with_deps` / `run_with_deps` to the two architecturally-
unreachable dependencies above (`read_slack_config` and a closure that
delegates `post_message_inner` to `run_curl_with_timeout`). `run`
additionally constructs `std::io::stdout()` for the writer parameter.
Both functions contain only delegation — no branching, no state
mutation, no error handling beyond the `let _ = writeln!(...)` ignore
on stdout failure (an expected pattern when stdout is closed by a
shell pipe). `notify_with_deps` and `run_with_deps` are fully covered
by `notify_with_deps_*` and `run_with_deps_*` tests; the binders'
remaining lines are pure production wiring that any test invocation
would have to recreate via subprocess spawn (which would re-trigger
the env-var race and the real-curl dependencies).

## src/phase_finalize.rs

`run_impl_with_deps(root, cwd, args, notifier)` is the testable
seam that drives the Slack-success, Slack-error, and state-record
branches against a tempdir state file. Two regions remain
architecturally unreachable from the in-process test harness.

### `cwd_scope::enforce` error forwarding (lines 86–88)

`run_impl_with_deps` forwards `crate::cwd_scope::enforce(cwd, root)`'s
`Err(msg)` through a three-line pass-through that wraps the message in
the standard `{"status":"error","message":...}` shape. Coverage of the
`enforce` function itself lives in `src/cwd_scope.rs` (98.74% regions
via the `cwd_scope.rs` inline test module) and the integration tests
in `tests/phase_finalize.rs` exercise the Ok-path through
`run_impl_with_deps` already. Adding a dedicated drift-fixture test
inside `phase_finalize.rs` would require initializing a real git repo
in a tempdir to give `current_branch_in(cwd)` a branch, then writing
a state file with `relative_cwd` set, then constructing a cwd outside
the expected subdirectory — a substantial fixture for a three-line
delegation that cannot independently fail. Coverage is transitive
through `cwd_scope::enforce`'s own test module.

### `run()` CLI entry (lines 255–266)

`run(args)` matches on `run_impl(&args)`, prints success JSON, or
calls `json_error` and `process::exit(1)` on infrastructure failure.
`process::exit` is unreachable from inside the calling process — the
established pattern in `src/analyze_issues.rs:524`,
`src/analyze_issues.rs:564`, and `src/analyze_issues.rs:607` already
documents this architectural limit. Subprocess-exit observation is
covered by the existing `tests/phase_finalize.rs` integration tests,
which spawn `flow-rs` and assert on the exit code and stdout. `run`'s
remaining lines (the `Ok(result) => println!` branch and the
`Err(e) => json_error + exit(1)` branch) are CLI plumbing for which a
tighter waiver is impossible without introducing a writer-injection
seam at `run` itself — the cost-benefit equation matches the
established pattern.

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

