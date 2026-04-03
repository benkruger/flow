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
