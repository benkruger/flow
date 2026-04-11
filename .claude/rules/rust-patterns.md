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
legacy state files while allowing the operation to proceed with
empty data rather than failing entirely.

## Hook Input Boolean Field Tolerance

Never guard with `value.as_bool() == Some(true)` alone in
security-enforcement hooks. Write a defensive `is_truthy` helper that
accepts bool, string `"true"`/`"1"`, and non-zero numbers.

## CLI Testability — run_impl Pattern

Extract a fallible `run_impl(args: &Args) -> Result<T, String>` and
make `run()` a thin wrapper that calls it and `process::exit(1)` on
`Err`. `process::exit` terminates the test process, so error-path
tests must target `run_impl`.

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
when state files carry corrupt or legacy values (hand edits, external
writers, or integer overflow from a prior session). Raw `+ 1` or
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
every entry the filesystem considers present. The installer then
skips the path without writing, preserving whatever is already there.

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
itself, never its target. Citing this rule for a deletion-path concern
is a false positive. For the separate concern of iterating a directory
and deleting entries, see "Safe Directory Iteration and Deletion"
below.

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
   existed but every attempt failed (`"failed: <first_error>"`). Do
   NOT use `"deleted"` when only some matches were removed and
   others failed — that hides the failures. The first-error-reporting
   shape (only report failure when NO file was deleted) balances
   signal strength against noise: a single transient error does not
   block the entire cleanup, but a hard failure is still surfaced.

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

**Plan phase checklist for `fs::read_dir` + delete loops.** When a
plan task describes a helper that iterates a directory and deletes
matching entries, enumerate these three risks explicitly in the
Risks section before Code Review catches them:

- Non-file entries that happen to match the filter prefix (directories,
  sockets, named pipes) — must be skipped, not deleted, not aborting
  the loop
- Partial failure aggregation — loop must continue past individual
  errors so one bad entry cannot orphan the others
- Return shape for partial success — distinct statuses for
  no-match, any-deleted, all-failed

**Test coverage for directory iteration helpers.** Every new helper
of this shape must ship with tests covering:

- Single matching file → `"deleted"`, file gone
- No matching files → `"skipped"`
- Multiple matching files → `"deleted"`, all gone
- Directory entry matching the prefix alongside real files → directory
  untouched, files still deleted, step returns `"deleted"`
- Missing target directory (`read_dir` returns `Err`) → `"skipped"`,
  no panic
- Branch-scoped or prefix-scoped isolation (concurrent callers with
  different prefixes do not interfere)
- Trailing-separator precision when the prefix ends in a
  character-class boundary (e.g., `"foo."` must not match `"foo_bar"`)

## Guard Universality Across CLI Entry Points

When adding a process-level guard (recursion check, cwd drift check,
permission check) to ONE entry point in a CLI command family, the
same guard must be added to every sibling entry point in the same
family. FLOW has two relevant families:

- **CI-tier runners:** `bin/flow ci`, `bin/flow build`, `bin/flow lint`,
  `bin/flow format`, `bin/flow test` (`src/ci.rs`, `src/build.rs`,
  `src/lint.rs`, `src/format_check.rs`, `src/test_runner.rs`).
- **State mutators:** `bin/flow phase-enter`, `bin/flow phase-finalize`,
  `bin/flow phase-transition`, `bin/flow set-timestamp`,
  `bin/flow add-finding`, `bin/flow add-issue`,
  `bin/flow add-notification`, `bin/flow append-note`
  (`src/phase_enter.rs`, `src/phase_finalize.rs`, the
  `PhaseTransition` branch in `src/main.rs`, `src/commands/*.rs`,
  `src/add_finding.rs`, etc.).

**Read-only exemption.** Subcommands that only READ the state file
and plan/worktree files (no mutations, no tool dispatch) are
exempt from `cwd_scope::enforce` — a wrong cwd on a read-only
command cannot drift the flow because the command produces no
side effects. The current exempt set is:

- `bin/flow format-status` (`src/format_status.rs`) — renders the
  status panel from state
- `bin/flow tombstone-audit` (`src/tombstone_audit.rs`) — scans
  `tests/*.rs` for tombstone PR references and queries GitHub
- `bin/flow plan-check` (`src/plan_check.rs`) — runs the
  scope-enumeration scanner against the current plan file

When adding a new read-only subcommand, add it to this list AND
to the corresponding list in CLAUDE.md's Subdirectory Context
section so the two canonical enumerations stay in sync.

Before merging a PR that adds a guard, grep `src/main.rs` for every
`Commands::` variant in the target family and verify the guard lands
in every `run_impl` or `run()` entry. A guard that exists in only one
runner creates divergent behavior: the user hits the same failure
mode in the unguarded sibling. The class of bug is invisible to
individual unit tests — only a contract test that enumerates every
variant can catch it mechanically. Consider adding such a contract
test whenever a new guard is introduced.

When tests spawn `CARGO_BIN_EXE_flow-rs` subprocesses while the test
suite itself is running inside a `bin/flow ci` invocation,
`FLOW_CI_RUNNING=1` is inherited from the parent and recursion guards
on the child will fire. Tests in this situation must call
`.env_remove("FLOW_CI_RUNNING")` on the `Command` to simulate a
fresh invocation.

The two family lists above are also the canonical enumeration used
by `.claude/rules/scope-enumeration.md` — the prose-side rule that
requires every universal-quantifier claim about a code family to
carry a named sibling list. When you add a new member to either
family, update both this section and any plan prose that references
the family by its universal noun so the named list stays in sync.

## Local Doc Comments

Any non-obvious design decision (custom formatters, shared constants,
unusual return types) must have a local doc comment on the definition
site summarizing why it exists in one sentence.

## Test Module Section Markers

Group related tests inside a `#[cfg(test)] mod tests` block using
single-topic section markers: `// --- primary_name ---` where
`primary_name` is the core function or feature being tested. When a
test group covers multiple related functions (e.g. a helper and its
wrapper), use the top-level abstraction name, not a slash-separated
list or a parenthesized signature.

- Correct: `// --- tolerant_i64 ---` (covers `tolerant_i64` and
  `tolerant_i64_opt`)
- Wrong: `// --- tolerant_i64_opt() / tolerant_i64() ---`
- Wrong: `// --- tolerant_i64(v: &Value) ---`

Before adding a new marker, grep the file for existing `// --- ` lines
and match their style. A marker that deviates from the file's
convention is a maintainability regression — the pattern is
discoverable only by reading the file, so consistency matters.

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
