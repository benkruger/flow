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

## Default Value Handling

Python `dict.get(key, default)` returns a default when the key is
absent. Rust `serde_json::Value::get(key)` returns `Option<&Value>`
with no default parameter. When porting a Python function that uses
`dict.get()` with a default, apply the same default in Rust via
`.unwrap_or()` or `.unwrap_or_else()`. Omitting the default changes
error behavior — the Python code succeeds while the Rust code fails.

## Subprocess Timeout Parity

When Python uses `subprocess.run(timeout=N)`, the Rust port must
preserve the same timeout. Omitting a timeout changes failure
behavior — the Python call raises `TimeoutExpired` after N seconds,
but the Rust call blocks indefinitely. Use `run_cmd` with
`Some(Duration::from_secs(N))` or implement a polling-based timeout
via `try_wait()`. Audit every `subprocess.run` call with a `timeout`
parameter during the port — missing timeouts are silent regressions
that only manifest under network failures or API outages.

## Python Bridge Pattern

When a ported script still has Python callers that import its
functions, the bridge module needs two functions: a subprocess
delegate (`append_log`) for callers in other lib scripts, and
a direct Python fallback (`_direct_append`) for `main()`. The
fallback prevents infinite recursion when `bin/flow` dispatches
to the Python script and the Rust binary is absent. Document
which function is for which context with inline comments.