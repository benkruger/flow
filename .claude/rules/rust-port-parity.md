# Rust Port Parity

When porting Python functions to Rust, verify JSON serialization
produces identical output — not just identical parsed values.

Python dicts preserve insertion order. Rust `HashMap` does not.
Use `IndexMap` (with serde feature) for any map that will be
serialized to JSON where key order must match the Python output.

When a plan adopts a map type for a Rust port, include a task
that serializes the output and compares key order against the
Python equivalent. Key-order divergence is a correctness bug,
not tech debt — downstream consumers depend on identical output.

When using `serde_json::Value` for dynamic JSON manipulation
(e.g. `mutate_state` with untyped closures), enable the
`preserve_order` feature in `Cargo.toml`. Without it,
`serde_json::Map` uses `BTreeMap` which alphabetically sorts
keys on every round-trip — silently reordering state files.

## String Slicing

Python `len()` counts code points, `s[:N]` slices by code point.
Rust `str::len()` counts bytes, `&s[..N]` slices by byte — panics
if the boundary falls inside a multi-byte UTF-8 character. Use
`s.chars().count()` for length and `s.chars().take(N).collect()`
for truncation when porting Python string slicing.

When fixing byte-offset slicing in a function, audit every slice
and index operation in the function — not just the ones named in
the issue or plan. Intermediate slices like `s[..pos]` where `pos`
comes from `find` or `rfind` are safe only when the string is
guaranteed ASCII. Document the invariant with an inline comment
when leaving a byte-index slice in place.

When writing Rust tests for char-count-bounded functions, assert
`result.chars().count() <= N` — not `result.len() <= N`. The
distinction documents the invariant the code enforces, even when
both are equivalent for ASCII output.

## Regex Lookbehind/Lookahead Not Supported

Python `re` supports lookbehinds `(?<!\\);` and lookaheads `(?=foo)`.
Rust's `regex` crate does not — compiling a pattern with lookaround
returns `Err(Syntax(look-around ... is not supported))` at runtime.
When porting a Python regex with lookaround, replace it with
byte-level scanning: iterate `command.as_bytes()` and check
`bytes[i-1]` manually for the condition the lookbehind expressed.

Example: Python `(?<!\\);` (unescaped semicolon) becomes a loop
that returns true for any `;` where `i == 0 || bytes[i-1] != b'\\'`.

Pure byte scanning is safe for ASCII operators (`;`, `>`, `&`, `|`)
which is the typical use case for shell command validation. For
non-ASCII contexts, use `fancy-regex` crate (which supports
lookaround) or restructure the pattern to avoid lookaround.

## Default Value Handling

Python `dict.get(key, default)` returns a default when the key is
absent. Rust `serde_json::Value::get(key)` returns `Option<&Value>`
with no default parameter. When porting a Python function that uses
`dict.get()` with a default, apply the same default in Rust via
`.unwrap_or()` or `.unwrap_or_else()`. Omitting the default changes
error behavior — the Python code succeeds while the Rust code fails.

## CLI Argument Group Parity

Python `argparse.add_mutually_exclusive_group(required=True)` rejects
invocations that omit all group members. Clap's `group = "action"` on
individual args creates a mutually exclusive group but does not make it
required — both booleans default to false and the command silently
proceeds. Use a struct-level `ArgGroup` with `.required(true)` to match
the Python behavior. Audit every `add_mutually_exclusive_group` call in
the Python source for `required=True` during the port.

## Exec Target Parity

When Python uses `os.execvp` to call `bin/flow` (the hybrid
dispatcher), the Rust port must also exec into `bin/flow` — not
`flow-rs` (the raw binary). The dispatcher provides Python
fallback for subcommands not yet ported to Rust. Exec'ing
`flow-rs` directly causes exit 127 for unported subcommands
with no fallback. Locate `bin/flow` by traversing up from
`current_exe()` (3 levels: binary → release → target → root)
then into `bin/flow`.

## Subprocess Timeout Parity

When Python uses `subprocess.run(timeout=N)`, the Rust port must
preserve the same timeout. Omitting a timeout changes failure
behavior — the Python call raises `TimeoutExpired` after N seconds,
but the Rust call blocks indefinitely.

Never use `try_wait()` polling followed by `wait_with_output()`.
The `try_wait()` call reaps the child process on success, and
`wait_with_output()` internally calls `wait()` again — which
fails with ECHILD on an already-reaped process, silently
discarding all stdout data. Additionally, if stdout is piped
but never drained, the child process blocks when the pipe buffer
fills (typically 64KB), causing `try_wait()` to never return and
the timeout to fire on every invocation with large output.

The correct pattern: take `child.stdout` before the poll loop,
drain it in a spawned thread, poll `try_wait()` for exit status,
then join the reader thread to get the buffered output.

### Reference Implementation Verification

When writing a new subprocess runner by modeling it on an existing
module, verify the reference module complies with this section
BEFORE copying its shape. Several pre-existing modules
(`src/finalize_commit.rs`, `src/notify_slack.rs`,
`src/close_issue.rs`, `src/close_issues.rs`, `src/issue.rs`,
`src/start_setup.rs`, `src/cleanup.rs`) were ported before this
rule was codified and use the prohibited `try_wait()` +
`wait_with_output()` pattern. The compliant reference is
`src/analyze_issues.rs` lines 472-518, which uses the thread-drain
pattern. Grep the candidate module for `wait_with_output` before
adopting its runner as a template — presence of that call means
the module is non-compliant and unsafe to copy.

The broader principle applies to any rule in this file: a reference
implementation predating the rule cannot be trusted. Verify before
copying.

## Python Bridge Pattern

When a ported script still has Python callers that import its
functions, the bridge module needs two functions: a subprocess
delegate (`append_log`) for callers in other lib scripts, and
a direct Python fallback (`_direct_append`) for `main()`. The
fallback prevents infinite recursion when `bin/flow` dispatches
to the Python script and the Rust binary is absent. Document
which function is for which context with inline comments.

## Subprocess CWD Parity

When porting a Python function that calls `subprocess.run(..., cwd=path)`
to Rust, pass the equivalent directory to `Command::current_dir(path)`.
Omitting it causes the subprocess to inherit the parent process CWD,
which in tests means the test binary's CWD (typically the host repo
root) rather than the fixture directory. Symptom: tests that pass
when run from a fresh temp dir fail when run as part of the full suite
because `current_dir` differs. Always audit subprocess calls in the
Python source for `cwd=` and mirror them in Rust `Command::current_dir()`.

### Inherited CWD — Use current_dir(), Not project_root()

The opposite direction also matters: when a Python script's
`subprocess.run(...)` calls pass NO `cwd=` argument, the script
inherits the caller's working directory implicitly. The Rust port
MUST match that by reading `std::env::current_dir()` — NOT by
calling `project_root()`.

Why: `project_root()` returns the first entry of `git worktree list
--porcelain`, which is always the MAIN repo root. When a FLOW phase
invokes a per-branch script from a linked worktree (which is how
every FLOW phase operates), `project_root()` sends git commands to
the main worktree where `HEAD=main`. A freshness check like
`git merge-base --is-ancestor origin/main HEAD` then trivially
succeeds — `main` is an ancestor of itself — and the feature
branch's actual state is never checked. The Python original
inherits the worktree CWD via `subprocess.run` default behavior;
the Rust port must match.

`project_root()` is the right choice for scripts that operate on
shared `.flow-states/` paths regardless of caller location. It is
the WRONG choice for scripts that run git commands against the
caller's branch.

How to apply: when porting a Python script, grep its `subprocess.run`
calls for `cwd=`. If none pass it, the script inherits CWD. Use
`std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))`
and pass that to every `Command::new(...).current_dir()` call.
Integration tests must set up a LINKED worktree (`git worktree add`)
and run the binary from inside it — tests that use standalone repos
with no linked worktrees cannot distinguish `project_root()` from
`current_dir()` and will falsely pass.

## Test-Module Subprocess Stdio

Cargo's test harness does not capture inherited child-process file
descriptors. Unlike pytest, which captures child stdio by default,
`cargo test` only captures Rust's own `println!` output — anything a
spawned subprocess writes to the inherited stdout/stderr goes straight
to the terminal. Rust tests that call `Command::...status()` without
explicit stdio redirection leak every byte the child writes: git's
`Initialized empty Git repository`, `[main (root-commit) <sha>]` commit
lines, `Switched to a new branch`, the detached HEAD advice block, and
any script `echo` the fixture uses. When porting Python tests that used
`subprocess.run` (which pytest captured implicitly), the Rust port must
redirect stdio explicitly — do not rely on implicit capture.

The standard fix is `Command::output()`. It captures and drops
stdout/stderr and returns an `Output { status, stdout, stderr }` whose
`.status.success()` check is a drop-in replacement for the existing
`.status().unwrap().success()` pattern. No new imports are required —
`Output` is in `std::process` alongside `Command`. The diff per
callsite is mechanical: `.status()` → `.output()` and, where the
closure asserts on the return value, `status.success()` →
`output.status.success()`. Pipe deadlock is not a concern for the
typical fixture workload: `Command::output()` drains stdout/stderr
internally, and git/fixture scripts emit well under the 64 KB pipe
buffer.

For the rare test that intentionally routes output through a
stdio-inheriting production function (e.g., a trip-wire verifying that
child stdout cannot corrupt a json-literal return value), do not
override the production function's stdio contract. Instead, have the
test's child script self-redirect at the shell level —
`echo 'msg' > /dev/null 2>&1` — so the bytes never reach the inherited
fd. Update the test's doc comment to explain the invariant the test is
protecting and why the redirect preserves the trip-wire.

Never introduce a dependency on implicit cargo capture. The Python
originals did not leak only because pytest captured child fds; there
is no equivalent guarantee in Rust. Every subprocess callsite in a
`#[cfg(test)]` module is the author's responsibility.

## CLI Testability — Extract run_impl

When a Rust port's plan requires CLI error-path tests (missing
state file, corrupt JSON, happy-path JSON shape), extract a
fallible `run_impl(args: &Args) -> Result<T, String>` helper
and make `run()` a thin wrapper that calls `run_impl` and
`process::exit(1)` on `Err`. `process::exit` terminates the
test process, so any error-path test of `run()` directly is
impossible — the tests must target `run_impl`.

Why: existing modules like `format_issues_summary.rs` embed
`process::exit` directly in `run()`. When a plan says "follow
that pattern" AND lists `test_cli_missing_state_file` or
`test_cli_corrupt_state_file` by name, the two requirements
conflict. Extract `run_impl` as the testable layer so the plan
can have both pattern parity and coverage.

How to apply: at the start of the Code phase, before writing the
first test, check whether the plan enumerates CLI error-path
tests. If yes, refactor `run()` to delegate to `run_impl` as the
first implementation step — writing the tests against a
non-existent `run_impl` is a natural TDD cycle.

## Test Naming — cli_ Prefix Contract

Tests prefixed `test_cli_*` must exercise `run` or `run_impl` —
not the pure format function. Tests that call only the pure
formatter should drop the `cli_` prefix.

Why: the `cli_` prefix signals that a test covers the CLI entry
point's argument parsing, file I/O, and error handling. A test
named `test_cli_writes_output_file` that calls the format
function and writes the file manually misleads future readers
about what the CLI layer is actually verified to do.

How to apply: when adding a test to a Rust port module, decide
first whether it covers CLI behavior (invoke `run_impl` with an
`Args` struct) or format behavior (invoke the pure function
directly). Name accordingly.

## Dead Changed-Flag Pattern

When porting Python code that uses a `changed` flag (or `modified`,
`dirty`, etc.) to decide whether to write back to disk, verify whether
the Rust equivalent writes unconditionally. If so, drop the flag
entirely — do not carry it forward as `_changed`.

Why: Python's file-persistence pattern tracks state mutations to avoid
unnecessary writes. Rust's `mutate_state()` acquires an exclusive lock
and writes unconditionally, so the flag is dead code. The leading
underscore suppresses the Rust `unused_variable` warning, which hides
the dead code from the compiler.

How to apply: when translating a function that mutates state, check the
closure's write semantics. If it writes every time, remove the flag
and the conditional writes. Do not preserve the flag "just in case" —
that is a false preservation of Python semantics.

## Sentinel Return Values — Document the Contract

When a ported function returns a sentinel value (empty vec, `None`,
`null`) to signal a condition to its caller, document the sentinel
contract in the function's doc comment. Never place an inline comment
above the return statement that describes the caller's fallback as if
it were the function's behavior.

Why: the Python original often inlined the check-and-fallback in one
place. When split across function and caller in Rust, the sentinel
contract lives in two places. Misleading inline comments at return
sites mislead readers about what the function actually returns.

How to apply: in the doc comment at the function's top, state what
each return value means to the caller. Comments at return sites should
describe the return value, not the caller's interpretation of it.

## Branch-Resolution Function Parity

Python `flow_utils.resolve_branch()` scans `.flow-states/` for a
unique state file when the current git HEAD does not match any
branch-named state file. Python `flow_utils.current_branch()`
returns only the exact git HEAD. When porting a hook or script
from Python, check which function the Python original called
and use the matching Rust equivalent — `git::resolve_branch()`
or `git::current_branch()`. Mismatching silently loses state
updates in worktree configurations where the shell's git HEAD
differs from the active flow's branch.

Audit every Python `resolve_branch()` call during a port. Hooks
that fire from any shell (Stop, StopFailure, PostCompact) almost
always need `resolve_branch()` because the user's shell cwd may
not match the active flow branch.

CLI subcommands that resolve a branch from an explicit cwd (e.g.
`bin/flow ci` running in a worktree) must use `git::resolve_branch_in(
override, cwd, root)` — the cwd-scoped variant — rather than
`current_branch_in(cwd)`. The `_in` variant preserves the state-file
scan fallback while reading the branch from the supplied directory
instead of the process cwd.

## Plan-Phase Cross-Checks for Rust Ports

The Plan phase for a Rust port must cross-check this document
before finalizing the task list. Mis-specifying a function at plan
time lets the bug pass through Code phase (tests agree with the
wrong function) and surfaces only during Code Review agent passes.

For every function the plan calls out by name, verify:

- **Branch resolution:** If the Python original reads a branch from
  the filesystem, identify whether it called `current_branch()` or
  `resolve_branch()`. Pick the matching Rust equivalent. In cwd-scoped
  contexts, pick `current_branch_in` or `resolve_branch_in` to match.
- **Subprocess timeout:** If the Python original passes `timeout=N`
  to `subprocess.run` or `proc.wait`, the Rust port must preserve
  the same timeout via `try_wait()` polling against a deadline.
- **Subprocess cwd:** If the Python original passes `cwd=path` to
  `subprocess.run`, the Rust port must call
  `Command::current_dir(path)` on every subprocess.
- **Upfront guards:** See "Upfront Guards Belong in run_impl" below.
- **Test-module subprocess stdio:** If the port adds Rust integration
  tests that spawn subprocesses, every `Command` call in a
  `#[cfg(test)]` module must use `.output()` (or explicit
  `Stdio::null()`) — never inherited `.status()`. Cargo's test harness
  does not capture inherited child fds (unlike pytest), and leaked
  stdout drowns CI output. See "Test-Module Subprocess Stdio" above.

Add a concrete task to the plan: "Cross-check rust-port-parity.md
sections Branch-Resolution Function Parity, Subprocess Timeout
Parity, Subprocess CWD Parity, Upfront Guards, and Test-Module
Subprocess Stdio against the Python source." The check is one read
of the Python source plus one read of this document — it takes
minutes and catches the exact class of bug the review agents found.

## Upfront Guards Belong in run_impl

When the Python original performs a single upfront check in `main()`
before dispatching to sub-functions (e.g. `if not bin_ci.exists():
print(error); sys.exit(1)` in `lib/ci.py` before calling either the
retry or non-retry path), the Rust port must place that guard in
`run_impl` — not in the individual sub-functions.

Why: placing the guard in only one sub-function silently diverges
error behavior across dispatch paths. The non-retry path returns
the Python-parity error message; the retry path fails later at
`Command::spawn()` and returns a different error message. Tests
that only cover the non-retry path miss the divergence entirely.

How to apply: when reading the Python source at plan time, locate
every guard in `main()` that runs before dispatch. For each guard,
add an explicit task to the plan: "Guard X runs in run_impl before
the retry/non-retry dispatch." When writing the test list for
run_impl, include a test case that exercises each guard through
both dispatch paths — a single test that asserts both paths return
the same error message is sufficient, but the test must exercise
both dispatch paths.

The inverse mistake is also possible: placing the guard in
`run_impl` when it belongs in a sub-function (e.g. when only one
dispatch path depends on the guard). Verify which path the Python
main() checks the guard for — if it checks before both, the guard
is upfront; if it checks only before one, the guard belongs with
that sub-function.

## State Mutation Object Guard

`serde_json::Value::IndexMut` for string keys panics on arrays,
bools, numbers, and strings — only objects and null values
(which auto-convert to empty objects) accept `state["key"] = v`.
Every `mutate_state` closure that assigns to string keys must
guard its mutations with `if !(state.is_object() || state.is_null())
{ return; }` to fail-open on corrupt or unexpected state files.
Without the guard, a state file that was manually edited to an
array, foreign-edited, or partially written during a crash causes
the hook to panic with exit 101 — breaking the fail-open contract
that hooks must never disturb the user's session.

## Empty-String vs Missing-Key Falsy Equivalence

Python's truthy check `if x:` treats both missing keys (via
`dict.get()` returning `None`) and empty strings (`""`) as falsy.
Rust's `Option<String>` treats `Some("".to_string())` as a valid
value distinct from `None`. When porting Python's `if x and y:`
or `if x:` patterns that gate on string values, filter empty
strings explicitly in Rust: `.and_then(|v| v.as_str())
.filter(|s| !s.is_empty())`. Missing this filter silently changes
semantics — a flow that blocked under Python now allows stop,
or vice versa.

## Counter Field Type Tolerance

State files can outlive the code that writes them. A counter
field like `compact_count` might have been written by an older
Python version as an integer, a newer version as a float (after
integer arithmetic), or a corrupted edit as a string. Rust ports
must accept all three numeric representations when reading
counters to avoid silently resetting to 1:

```rust
state.get("compact_count")
    .and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_f64().map(|f| f as i64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
    .unwrap_or(0)
```

Use `as_i64()` alone only for fields where you control both the
writer and reader in the same codebase generation.
