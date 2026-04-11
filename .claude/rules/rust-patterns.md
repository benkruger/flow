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

## Local Doc Comments

Any non-obvious design decision (custom formatters, shared constants,
unusual return types) must have a local doc comment on the definition
site summarizing why it exists in one sentence.

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
