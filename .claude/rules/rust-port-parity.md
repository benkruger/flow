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

## Default Value Handling

Python `dict.get(key, default)` returns a default when the key is
absent. Rust `serde_json::Value::get(key)` returns `Option<&Value>`
with no default parameter. When porting a Python function that uses
`dict.get()` with a default, apply the same default in Rust via
`.unwrap_or()` or `.unwrap_or_else()`. Omitting the default changes
error behavior — the Python code succeeds while the Rust code fails.

## Exec Target Parity

When Python uses `os.execvp` to call `bin/flow` (the hybrid
dispatcher), the Rust port must also exec into `bin/flow` — not
`flow-rs` (the raw binary). The dispatcher provides Python
fallback for subcommands not yet ported to Rust. Exec'ing
`flow-rs` directly causes exit 127 for unported subcommands
with no fallback. Locate `bin/flow` by traversing up from
`current_exe()` (3 levels: binary → release → target → root)
then into `bin/flow`.

## Python Bridge Pattern

When a ported script still has Python callers that import its
functions, the bridge module needs two functions: a subprocess
delegate (`append_log`) for callers in other lib scripts, and
a direct Python fallback (`_direct_append`) for `main()`. The
fallback prevents infinite recursion when `bin/flow` dispatches
to the Python script and the Rust binary is absent. Document
which function is for which context with inline comments.
