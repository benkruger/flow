# State Files

## Edit Tool Safety

Never use `replace_all=True` on JSON state file edits when the
`old_string` appears in multiple semantic contexts. "pending"
appears in both task statuses and phase statuses. Use targeted
`old_string` with enough surrounding context to make the match
unique to a single location.

## Numeric Fields

Store numeric state fields as raw integers, never formatted
strings. `cumulative_seconds` and `visit_count` must be integers
in the JSON state file. The human-readable format (e.g. `"<1m"`,
`"5m"`) is for display output only and must never be written to
storage.

When reading counters, use defensive tolerance for legacy state
files that may contain string or float representations (see
`rust-patterns.md` Counter and State Field Type Tolerance). Writers
must produce integers; readers must tolerate alternatives.

## Corruption Resilience

State files can become malformed through interrupted writes, kill
signals, filesystem errors, or external edits. Every function that
reads state files must handle corruption gracefully:

- **Empty file (0 bytes)** — treat as parse error and return `Err`.
  Do not write to the file (it may be mid-creation by another
  process).
- **Non-JSON content** — return `Err` and leave the file unchanged.
  `mutate_state` already handles this via the `serde_json::from_str`
  error path.
- **Wrong root type (array, string, number)** — `mutate_state`
  callers guard with `if !(state.is_object() || state.is_null())`.
  Functions that access nested keys (e.g. `state["phases"]`) add
  per-level guards that reset wrong types to empty objects (see
  `rust-patterns.md` State Mutation Object Guards).
- **Missing or wrong-type nested fields** — use `get()` chains
  with `and_then()` rather than `IndexMut` when reading. Use
  `tolerant_i64()` for counter fields. Auto-vivification via
  `IndexMut` is acceptable for writes but not for reads where the
  absence of a key has meaning.

When adding a new state-touching function, include edge case tests
for at least: missing file, empty file, and wrong-type fields the
function accesses.
