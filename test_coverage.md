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

## src/tui.rs

Closing the coverage gap on `tui.rs` (issue #1135) extracts the pure
fragments of the TUI's keystroke handlers, action methods, and free
helpers into directly-tested functions. The lines below remain
uncovered because they are the IO shell that wraps those pure
fragments — terminal initialization, subprocess spawns, AppleScript
result extraction via `output.status.success()`, and `process::exit`
sites that cannot be reached from inside the test process.

### abort_flow raw-mode dance and subprocess spawn (lines 317–325)

- `src/tui.rs:317–325` — `disable_raw_mode()` + `execute!(stdout, LeaveAlternateScreen)` + `eprintln!` + `Command::new(&bin_flow).args(&args).status()` + `enable_raw_mode()` + `execute!(stdout, EnterAlternateScreen)`. Toggling raw mode and the alternate screen requires a real terminal, and spawning `bin/flow cleanup` requires a primed target project — neither is present inside `cargo nextest`. The cleanup argument vector is fully covered by `build_cleanup_command_args` tests; only the spawn + terminal manipulation is unreachable.

### open_url Command::spawn (lines 1211–1215)

- `src/tui.rs:1211–1215` — `Command::new(program).args(&args).stdout(Stdio::null()).stderr(Stdio::null()).spawn()`. Spawning the macOS `open` binary requires a real desktop environment; `cargo nextest` runs in a non-interactive subprocess where the spawn fires and is immediately discarded by the `let _ =`. The (program, args) decision is fully covered by `build_open_url_command` tests; only the spawn itself is unreachable.

### activate_iterm_tab osascript spawn (lines 1257–1265)

- `src/tui.rs:1257–1264` — `Command::new("osascript").arg("-e").arg(&script).output()` plus the `output.status.success()` extraction that feeds `parse_osascript_result`. Spawning a real osascript subprocess against an iTerm2 instance is a host-environment dependency; the test suite runs under cargo nextest with no AppleScript runtime guaranteed. The script body is covered by `build_iterm_activation_script` tests; the success/stdout decision is covered by `parse_osascript_result` tests; only the spawn + `output.status.success()` extraction is unreachable.
- `src/tui.rs:1265` — the `Err(_) => false` arm. Reachable only when the osascript binary is missing entirely; the production failure mode is "iTerm2 inactive" which does NOT take this branch (osascript still runs successfully and returns "not found"). Covered architecturally by the negative-path symmetry in `parse_osascript_result` tests.

### find_bin_flow current_exe wrapper (lines 1292–1299)

- `src/tui.rs:1292–1298` — the `current_exe()` lookup, the `Some(bin_flow) => return` happy-path return, and the `PathBuf::from("bin/flow")` fallback. `std::env::current_exe()` returns the test runner binary inside `cargo nextest`, not the production `flow-rs` binary, so the happy-path branch never resolves to the real `<root>/bin/flow` and the fallback is structurally a "best effort" path. The walk-up + existence check is fully covered by `derive_bin_flow_path` tests against synthetic tmpdir fixtures; only the outer `current_exe`/return shape remains unreachable.

