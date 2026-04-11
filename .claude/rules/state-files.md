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
