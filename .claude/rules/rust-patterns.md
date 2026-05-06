# Rust Patterns

Durable Rust development patterns for the FLOW codebase. Covers JSON
serialization, string safety, state mutation guards, test conventions,
and CLI architecture patterns used across `src/*.rs` modules.

## JSON Key Order Preservation

Use `IndexMap` (with serde feature) for any map serialized to JSON where
key order matters. Enable `preserve_order` in `serde_json` Cargo.toml
features — without it, `serde_json::Map` uses `BTreeMap` which
alphabetically sorts keys on every round-trip, silently reordering
state files.

## String Slicing Safety

`str::len()` counts bytes, not code points. `&s[..N]` panics if the
boundary falls inside a multi-byte UTF-8 character. Use
`s.chars().count()` for length and `s.chars().take(N).collect()` for
truncation. When writing tests for char-count-bounded functions, assert
`result.chars().count() <= N` — not `result.len() <= N`.

## Regex Lookbehind/Lookahead

The `regex` crate does not support lookaround. Replace with byte-level
scanning: iterate `command.as_bytes()` and check `bytes[i-1]` manually.
Pure byte scanning is safe for ASCII operators (`;`, `>`, `&`, `|`).
For non-ASCII contexts, use the `fancy-regex` crate.

## Stateful Predicate-Based Scanners

When a byte-level scanner tracks state (quote context, escape state,
bracket depth) and accepts caller-supplied predicates, the scanner and
predicate must have an explicit contract about WHICH states the
predicate runs in and WHICH states the scanner matches internally.
`scan_unquoted` in `src/hooks/validate_pretool.rs` is the reference
implementation for this pattern.

**Contract requirements:**

- **Predicate state scope.** Document explicitly which scanner states
  invoke the predicate. `scan_unquoted` calls its predicate only in
  Normal state — quoted bytes are inert by construction.
- **Scanner-internal universal matches.** When a match is universal
  across multiple states (e.g. `$(` and backticks are structural in
  both Normal and Double state because bash expands them in both), the
  scanner itself must perform the match rather than relying on
  predicates to agree. Duplication invites drift.
- **Shared scanner, multiple predicates.** When the same state machine
  backs multiple CLI layers, both layers must go through the same
  scanner function so a quote-semantics bug fix in the scanner
  automatically applies to every layer.
- **Unclosed-state fallback.** When the scanner finishes inside a
  non-Normal state (unclosed quote, unterminated escape), return a
  distinct `Err(ScanError::Unclosed)` variant rather than silently
  returning `Ok(None)`. An unclosed quote is malformed input and
  could otherwise hide a structural operator from the scanner — a
  security-relevant bypass vector.

**How to apply:**

1. Enumerate every scanner state the predicate will run in. Pick one
   (typically "outside all non-literal contexts") and document it.
2. List every operator class the scanner must catch. Split them into
   "predicate-supplied" (caller-customizable) and "universal"
   (hardcoded in the scanner). Universal matches go in the scanner.
3. Treat the state machine as shared infrastructure, not caller-owned.
   Do NOT let consumers bring their own state machine — that defeats
   the shared-scanner guarantee.
4. Add an explicit error variant for each malformed-input class.

## State Mutation Object Guards

`serde_json::Value::IndexMut` for string keys panics on arrays, bools,
numbers, and strings. Every `mutate_state` closure that assigns to
string keys must guard with
`if !(state.is_object() || state.is_null()) { return; }`.

Nested assignments (`state["outer"]["inner"] = v`) require per-level
guards — check the type of each intermediate level before assigning.
When a nested field like `state["phases"]` must be an object for
downstream IndexMut access, reset it to `json!({})` if its type is
wrong. This auto-heal approach prevents panics from corrupted or
legacy state files.

## Hook Input Boolean Field Tolerance

Never guard with `value.as_bool() == Some(true)` alone in
security-enforcement hooks. Write a defensive `is_truthy` helper that
accepts bool, string `"true"`/`"1"`, and non-zero numbers.

## CLI Testability — run_impl Pattern

Extract a fallible `run_impl(args: &Args) -> Result<T, String>` and
make `run()` a thin wrapper that calls it and `process::exit(1)` on
`Err`. `process::exit` terminates the test process, so error-path
tests must target `run_impl`.

**Main-arm dispatch.** The same seam applies to `src/main.rs` match
arms whose body is more than a one-line delegation. When an arm owns
branch resolution, state-file IO, or validation that calls
`process::exit` directly, extract the body into the owning module as
`pub fn run_impl_main(params, root[, cwd]) -> (ReturnType, i32)` and
have the main arm call one of the centralized helpers in
`src/dispatch.rs`:

- `dispatch::dispatch_json(Value, i32)` — for subcommands whose
  stdout contract is JSON.
- `dispatch::dispatch_text(&str, i32)` — for subcommands whose
  stdout contract is plain text.

Return type choices:

- `(Value, i32)` — JSON-only stdout, no stderr path.
- `(String, i32)` — plain-text stdout, no stderr path.
- `Result<(Value, i32), (String, i32)>` — when the arm has a stderr
  error path. `Ok` routes to stdout via `dispatch_json`; `Err` goes
  to stderr with the paired exit code.

`run_impl_main` functions take `root: &Path` (and `cwd: &Path` where
the arm enforces cwd drift) as parameters rather than calling
`project_root()` / `current_dir()` internally, so integration tests
in `tests/<name>.rs` can pass a `TempDir` fixture without colliding
with the host worktree.

**Seam-injection variant for externally-coupled code.** When a
module's production wrapper depends on resources `cargo nextest`
cannot supply (real TTY, raw-mode terminal, network socket), expose
the dependencies as closure parameters in an `_impl` variant and
keep the production wrapper a thin closure-supplier.

**This carve-out is closed, not open-ended.** The list of
"externally-coupled" resources that justify a pub test seam is
exactly: real TTY (`libc::isatty`), raw-mode terminal (crossterm
`enable_raw_mode`/`LeaveAlternateScreen`), live crossterm event
loop, network socket opened inside the module. Nothing else. The
following do NOT qualify and cannot be used to justify a pub test
seam:

- Subprocess calls to `gh`, `git`, `bin/flow`, or any other binary —
  these are fixture-controllable (prepend a fake binary to PATH,
  use a bare git repo, spawn the real binary with prepared stdin).
- File system reads (state files, sentinel files, plan files,
  config files) — fixture-controllable via tempdir.
- Environment variables — controllable via `Command::env`.
- Return values of `current_branch()`, `project_root()`,
  `current_dir()` — fixture-controllable.
- CI sentinel state, tree snapshot comparison, PR status —
  fixture-controllable.

A `pub fn <name>_with_runner`, `_with_resolver`, `_with_deps`, or
`_inner` variant for any of those surfaces is a test seam, not an
externally-coupled seam. Per
`.claude/rules/test-placement.md` "Bright-line test for `pub`
additions", it is forbidden unless it has a named non-test consumer.

Reference:
`tui_terminal::run_tui_arm_impl(is_tty_fn, run_terminal_fn, root)`
accepts `is_tty_fn: FnOnce() -> bool` and `run_terminal_fn:
FnOnce(&mut TuiApp) -> io::Result<()>` so unit tests substitute
mock closures and assert each branch's return tuple. The production
wrapper `run_tui_arm` returns `!` and calls `run_tui_arm_impl` with
real implementations, then matches the Result to `process::exit`.

The same closure-injection pattern applies to RAII guards whose
release path needs unit-test verification: parameterize the cleanup
closure (`TerminalGuard<F: FnMut()>` with `release_fn: Option<F>`)
so unit tests construct a guard with a flag-setting closure, panic
inside `std::panic::catch_unwind`, and assert the flag was set on
Drop unwind.

**Three-tier dispatch for subprocess-coordinating modules.**

**NOTE:** this tier pattern has been over-applied in the codebase as
pub-for-testing. Before adopting it, confirm the dependencies truly
cannot be exercised via the real production path with fixtures. If
the dependencies are `gh`/`git`/`bin/flow` subprocess calls, fixture
them via a fake binary on PATH instead of introducing a `_with_deps`
pub seam. A `_with_deps` variant whose only non-test caller is the
same-module `run_impl` production binder fails the bright-line
test in `.claude/rules/test-placement.md` and is forbidden.

The pattern below is documented because it genuinely fits a narrow
class of modules (TUI, complex state machines driven by live
subprocesses whose failure modes cannot be reproduced by faking
stdout). Apply it only when the simpler-primitive and fixture-based
approaches have been tried and documented as insufficient.

1. `pub fn run_impl_with_deps(args, root, cwd, ...closures) -> Value`
   — testable core with injectable closures for every subprocess
   callout. Returns `Value` unconditionally when every failure mode
   can be represented as a `status: "error"` payload.
2. `pub fn run_impl(args) -> Value` (or `Result<Value, String>` when
   an infrastructure `Err` path is reachable) — production binder
   that supplies the real closures.
3. `pub fn run_impl_main(args, root, cwd) -> (Value, i32)` — main-arm
   dispatcher that wraps into the `(Value, i32)` contract.

**Exit code convention for business errors.** When `run_impl` returns
`Value` unconditionally, the paired `run_impl_main` wraps as
`(v, 0)` — exit code is always `0`. Callers distinguish success from
failure by parsing the JSON `status` field, not by shell exit code.
Exit code `1` is reserved for infrastructure failures that escape the
JSON contract.

## Test Subprocess Stdio

Cargo's test harness does not capture inherited child-process stdio.
Use `Command::output()` (captures and drops stdout/stderr) instead of
`Command::status()` in test modules. For tests that pipe stdin, use
`spawn() + wait_with_output()` with all three streams piped explicitly.

## Sentinel Return Values

Document sentinel return values (empty vec, `None`, `null`) in the
function's doc comment. Comments at return sites should describe the
return value, not the caller's interpretation.

## Branch-Resolution Functions

- `resolve_branch` — accepts `--branch` override, checks state file existence
- `current_branch` — simple current branch, no override
- `resolve_branch_in` — cwd-scoped variant for worktree contexts

## Counter and State Field Type Tolerance

State files can outlive the code that writes them. Accept int, float,
and string representations when reading counters.

`src/utils.rs` exposes two functions for this tolerance:

- `tolerant_i64_opt(v: &Value) -> Option<i64>` — primary form. Returns
  `None` when the value cannot be interpreted as a number. Use when the
  caller needs to distinguish "field missing / unparseable" from "present
  with value 0".
- `tolerant_i64(v: &Value) -> i64` — thin `unwrap_or(0)` wrapper over
  `tolerant_i64_opt`. Use for counter fields where a missing or
  unparseable value should mean zero.

When other modules need the same tolerance, import from `crate::utils`
— do not inline the fallback chain.

## Saturating Arithmetic on Counter Reads

Counter reads via `tolerant_i64` can return values at or near `i64::MAX`
when state files carry corrupt or legacy values. Raw `+ 1` or
`+ elapsed` arithmetic on those values panics in debug builds and wraps
silently to `i64::MIN` in release builds, corrupting the counter.

<!-- scope-enumeration: imperative -->
Use `saturating_add` at every counter-increment callsite:

```rust
let visit_count = tolerant_i64(&phase_data["visit_count"]).saturating_add(1);
let cumulative = existing.saturating_add(elapsed);
```

The helper itself cannot defend against this — the caller chooses the
arithmetic. Apply the guard wherever a counter read is followed by an
increment or accumulation.

## Empty-String vs Missing-Key Equivalence

`Some("".to_string())` is distinct from `None` in Rust. When porting
falsy checks, filter empty strings explicitly:
`.and_then(|v| v.as_str()).filter(|s| !s.is_empty())`.

## Glob Dot-Prefix Filtering

`*` patterns should not match entries starting with `.` (fnmatch
convention). Filter entries whose name starts with `.` unless the
pattern itself starts with `.`.

## Upfront Guards in run_impl

When a function performs a single upfront check before dispatching to
sub-functions, place that guard in `run_impl` — not in the individual
sub-functions. This avoids divergent error behavior across dispatch
paths.

## Symlink-Safe Existence Checks Before Writes

Never guard a file write with `Path::exists()` (or equivalent
`Path::try_exists()`, `Path::metadata()`) followed by `fs::write` or
any other file-creation call. `exists()` follows symlinks, so a
dangling symlink at the target path returns `false` — and the
subsequent `fs::write` then follows the symlink to write to its
pointed-at target, which can be anywhere on the filesystem the
current user has access to. This is a real symlink-escape bug surface
for any priming, templating, or install step that writes files into
user-controlled directories.

Use `fs::symlink_metadata(&path).is_ok()` for the existence check
instead. `symlink_metadata` does not follow symlinks, so it returns
`Ok` for files, directories, valid symlinks, AND dangling symlinks —
every entry the filesystem considers present.

```rust
// Correct
if fs::symlink_metadata(&target).is_ok() {
    continue; // file, dir, valid symlink, or dangling symlink — skip
}
fs::write(&target, &content)?;

// Wrong — dangling symlink would cause fs::write to escape the dir
if target.exists() {
    continue;
}
fs::write(&target, &content)?;
```

This pattern applies to every installer in `src/prime_setup.rs`,
`src/start_workspace.rs`, any `write_rule`-style helper, and any future
code that writes files into a user-owned directory tree. Test cases
must include a dangling-symlink scenario alongside the normal-file,
directory, and missing-path cases.

The rule is scoped to **writes and file-creation calls only**. Deletion
paths (`fs::remove_file`, `fs::remove_dir`) do not have the same
symlink-escape risk — `fs::remove_file` on a symlink removes the link
itself, never its target.

## Safe Directory Iteration and Deletion

When a helper iterates `fs::read_dir()` and deletes matching entries,
three correctness failure modes are easy to miss and must be handled
explicitly:

1. **Non-file entries matching the filter.** `fs::read_dir` yields
   files, directories, symlinks, and other filesystem entries. A
   directory whose name matches the filter prefix will match the
   filter test, but `fs::remove_file` on a directory returns
   `EISDIR`/`EPERM`. Check `entry.file_type()` before calling
   `fs::remove_file` and skip entries that are neither regular files
   nor symlinks. `fs::remove_file` on a symlink removes the link
   itself, so symlinks are safe to delete.
2. **Early return on first deletion error.** A loop that returns on
   the first `fs::remove_file` error leaves remaining matching
   entries on disk. When the iterator yields a non-file entry or
   hits a transient permission error before the real files, the loop
   aborts and every subsequent file is orphaned. Use a continue-past-
   error loop that tracks `any_matched`, `any_deleted`, and
   `first_error: Option<String>` across iterations.
3. **Partial success return shape.** With continue-past-error, the
   return value must distinguish three states: no matches (`"skipped"`),
   at least one file deleted successfully (`"deleted"`), and matches
   existed but every attempt failed (`"failed: <first_error>"`).

Canonical shape:

```rust
fn try_delete_matching(dir: &Path, prefix: &str) -> String {
    let entries = match fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(_) => return "skipped".to_string(),
    };
    let mut any_matched = false;
    let mut any_deleted = false;
    let mut first_error: Option<String> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(prefix) {
            continue;
        }
        // Skip non-file entries (directories especially) so they
        // don't abort the loop and they don't get deleted.
        let is_candidate = match entry.file_type() {
            Ok(ft) => ft.is_file() || ft.is_symlink(),
            Err(_) => false,
        };
        if !is_candidate {
            continue;
        }
        any_matched = true;
        match fs::remove_file(entry.path()) {
            Ok(()) => any_deleted = true,
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(format!("{}", e));
                }
            }
        }
    }
    if any_deleted {
        "deleted".to_string()
    } else if any_matched {
        format!("failed: {}", first_error.unwrap_or_else(|| "unknown".to_string()))
    } else {
        "skipped".to_string()
    }
}
```

**Plan phase checklist for `fs::read_dir` + delete loops.** Enumerate
these three risks explicitly in the Risks section before Code Review
catches them:

- Non-file entries that happen to match the filter prefix
- Partial failure aggregation
- Return shape for partial success

## Guard Universality Across CLI Entry Points

When adding a process-level guard (recursion check, cwd drift check,
permission check) to ONE entry point in a CLI command family, the
same guard must be added to every sibling entry point in the same
family. FLOW has two relevant families:

- **CI-tier runner:** `bin/flow ci` (`src/ci.rs`). The `--format`/
  `--lint`/`--build`/`--test` single-phase flags route through the
  same `ci::run_impl` entry, so the guard added once to `ci::run`
  covers every phase variant.
- **State mutators:** `bin/flow phase-enter`, `bin/flow phase-finalize`,
  `bin/flow phase-transition`, `bin/flow set-timestamp`,
  `bin/flow add-finding`, `bin/flow add-issue`,
  `bin/flow add-notification`, `bin/flow append-note`.

**Read-only exemption.** Subcommands that only READ the state file
and plan/worktree files (no mutations, no tool dispatch) are
exempt from `cwd_scope::enforce` — a wrong cwd on a read-only
command cannot drift the flow because the command produces no
side effects. The current exempt set is:

- `bin/flow format-status` (`src/format_status.rs`)
- `bin/flow tombstone-audit` (`src/tombstone_audit.rs`)
- `bin/flow plan-check` (`src/plan_check.rs`)

When adding a new read-only subcommand, add it to this list AND
to the corresponding list in CLAUDE.md's Subdirectory Context
section so the two canonical enumerations stay in sync.

Before merging a PR that adds a guard, grep `src/main.rs` for every
`Commands::` variant in the target family and verify the guard lands
in every `run_impl` or `run()` entry. A guard that exists in only one
runner creates divergent behavior.

When tests spawn `CARGO_BIN_EXE_flow-rs` subprocesses while the test
suite itself is running inside a `bin/flow ci` invocation,
`FLOW_CI_RUNNING=1` is inherited from the parent and recursion guards
on the child will fire. Tests in this situation must call
`.env_remove("FLOW_CI_RUNNING")` on the `Command` to simulate a
fresh invocation.

The two family lists above are also the canonical enumeration used
by `.claude/rules/scope-enumeration.md` — the prose-side rule that
requires every universal-quantifier claim about a code family to
carry a named sibling list.

## Cwd-Inside-Destructive-Path Guard

`cwd_scope::enforce` (above) protects against cwd DRIFTING away from
the worktree subdirectory the flow expects. The complementary risk
runs in the opposite direction: a subcommand whose execution removes
the caller's cwd as part of its work. If the caller's shell sits
inside the path about to be deleted, a successful run leaves the
shell in a nonexistent directory and every subsequent command emits
`getcwd: cannot access parent directories`.

The canonical example is `bin/flow complete-finalize`, which removes
the worktree as part of cleanup. Its `run_impl` self-gates with a
canonicalize-and-compare check at the very top of the function,
before any side effect:

```rust
if let (Ok(cwd_canon), Ok(worktree_canon)) = (
    std::env::current_dir().and_then(|p| p.canonicalize()),
    Path::new(&args.worktree).canonicalize(),
) {
    if cwd_canon == worktree_canon || cwd_canon.starts_with(&worktree_canon) {
        let root = project_root();
        return json!({
            "status": "error",
            "reason": "cwd_inside_worktree",
            "message": format!(
                "cd to {} before running complete-finalize",
                root.display()
            ),
        });
    }
}
```

Three properties matter:

- **Canonicalize before comparing.** On macOS, `tempfile::tempdir()`
  hides under `/private/var/...` symlinks; `git worktree list`
  reports the symlinked form; `current_dir()` resolves through the
  symlink. Without canonicalization on both sides the comparison
  silently fails to match.
- **Equality OR prefix.** A cwd nested inside the worktree
  (`<worktree>/<sub>/...`) must trigger the gate too. The
  `cwd_canon.starts_with(&worktree_canon)` check covers descendants;
  the `==` check covers the exact-root case.
- **Skip on canonicalize error, do not panic.** When either path
  can't be canonicalized, the guard falls through and lets downstream
  steps surface a more specific error.

The error envelope uses the same `(status, reason, message)` shape
as `cwd_scope::enforce`. The exit code stays at 0 per the
"Exit code convention for business errors" note above — the JSON
`status` field is the actual signal callers parse.

When adding a new subcommand whose execution path removes the
caller's cwd, plant the same guard at the top of its `run_impl`
before any side effect runs.

## Local Doc Comments

Any non-obvious design decision (custom formatters, shared constants,
unusual return types) must have a local doc comment on the definition
site summarizing why it exists in one sentence.

## Test Module Section Markers

Group related tests inside a `tests/<name>.rs` integration test
file using single-topic section markers: `// --- primary_name ---`
where `primary_name` is the core function or feature being tested.
When a test group covers multiple related functions (e.g. a helper
and its wrapper), use the top-level abstraction name, not a
slash-separated list or a parenthesized signature.

Tests live in `tests/<name>.rs` parallel to `src/<name>.rs` and
drive through the public interface per
`.claude/rules/test-placement.md`. Inline `#[cfg(test)]` blocks in
`src/*.rs` are prohibited; section markers therefore live in the
integration test file, not in the source file.

- Correct: `// --- tolerant_i64 ---` (covers `tolerant_i64` and
  `tolerant_i64_opt`)
- Wrong: `// --- tolerant_i64_opt() / tolerant_i64() ---`
- Wrong: `// --- tolerant_i64(v: &Value) ---`

Before adding a new marker, grep the test file for existing
`// --- ` lines and match their style.

## Session Log Message Format

When adding `append_log` calls to a Rust module, use
`[Phase N] module-name — step (status)` format. Derive the phase
number via `phase_number()` from `phase_config.rs` — never hardcode
it unless the module is phase-specific (e.g., Phase 6 modules that
only run during Complete). For modules called from multiple phases
(e.g., `finalize_commit`), read `current_phase` from the state file
at runtime. Guard `append_log` calls in modules where
`.flow-states/` may not exist (test fixtures): check directory or
file existence before calling. `append_log` creates the directory
if missing, which breaks test fixtures that deliberately omit it.
