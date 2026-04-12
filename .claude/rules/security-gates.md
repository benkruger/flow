# Security Gates

When a CLI subcommand or entry point guards an action against
caller input (phase name, outcome, file path, flag), the guard
must be robust to input variation and fail-closed on uncertainty.
A guard that silently accepts a whitespace-padded or BOM-prefixed
input is not a guard — it is a suggestion the caller can ignore
by accident.

This rule applies to every gate that reads string input from the
CLI or from a state file and decides whether to permit or reject
an action. Examples in this codebase: `code_review_filing_gate`
in `src/add_finding.rs`, `should_reject_for_code_review` in
`src/issue.rs`, the scope-enumeration scanner in
`src/scope_enumeration.rs`. Future gates should follow the same
discipline.

## Normalize Before Comparing

Any string input that participates in a gate decision must be
normalized before comparison:

1. **Strip NULs** with `.replace('\0', "")`. Embedded NULs from
   truncated writes or editor artifacts defeat byte-equality.
2. **Trim whitespace** with `.trim()`. Leading or trailing
   whitespace from CLI args or state-file padding defeats
   byte-equality.
3. **Lowercase with ASCII semantics** (`to_ascii_lowercase()`)
   when the comparison is conceptually case-insensitive. Phase
   names, outcome names, and command names in FLOW are all
   intended to be case-insensitive for robustness — a caller
   passing the wrong case is a caller bug, not an attack, but
   the gate defending against it is cheap.

Normalization runs on BOTH sides of the comparison: if you are
checking `input == "flow-code-review"`, either normalize
`"flow-code-review"` too or spell out that the right-hand side is
already normalized. Asymmetric normalization is the bug that
adversarial tests find.

Extract normalization into a named helper when multiple gates
share the same logic, so the contract is visible and reusable:

```rust
fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}
```

## Positive Allowlist, Not Negative Denylist

When a gate enforces "only values in set X are permitted during
context Y," encode it as a positive allowlist membership check,
not as a denylist of forbidden values. A denylist fails the
moment a new value is added to the domain — the new value
silently passes the gate.

Example (correct — positive allowlist):

```rust
const CODE_REVIEW_ALLOWED_OUTCOMES: &[&str] = &["fixed", "dismissed"];
if !CODE_REVIEW_ALLOWED_OUTCOMES.contains(&outcome_norm.as_str()) {
    return reject();
}
```

Example (wrong — denylist):

```rust
if outcome_norm == "filed" {
    return reject();
}
// A future "deferred" outcome silently passes.
```

The allowlist makes the rule's invariant explicit in code: "Code
Review accepts exactly these outcomes." The denylist encodes
"Code Review rejects exactly this outcome," which is weaker and
brittle to future additions.

## Fail Closed When State Is Unreliable

When a gate reads state from a file (e.g., `current_phase` from
`.flow-states/<branch>.json`), distinguish three input states:

1. **No file / empty content** → pass. The command is running
   outside an active flow. This is legitimate usage.
2. **Non-empty content that parses and contains the expected
   field** → apply the gate logic.
3. **Non-empty content that fails to parse, has the wrong root
   type, or is missing the expected field** → **fail CLOSED**.
   Return a rejection message explaining that the phase could
   not be determined. Silent fall-through to "gate passes"
   means a corrupted state file becomes an escape hatch.

Fail-closed semantics matter most when the state file signals
that a flow is active but the gate cannot tell which phase. A
kill signal, interrupted write, or hand edit that leaves the
file unparseable must not silently disable the gate.

The rejection message should name the failure mode (invalid JSON,
missing field, wrong type) and point the user at the override or
recovery path if one exists.

## Enumerate Bypass Variants Before Coding, Not After

When a plan task adds a string-input gate, the test task that
precedes it must enumerate bypass variants explicitly in the
plan's Risks or test-notes section. The adversarial agent will
find these variants during Code Review if the tests do not cover
them — which wastes a Code Review cycle on work the Plan phase
could have prevented.

Minimum variant checklist for every string-input gate:

1. **Whitespace** — leading, trailing, and interior whitespace
2. **Case** — UPPERCASE, MixedCase, lowercase (at least two
   variants if comparison is intended to be case-insensitive)
3. **Embedded NUL** — trailing `\0` and interior `\0`
4. **Type variants** (for state-file gates) — current_phase
   as number, boolean, null, array, missing key
5. **Encoding** (for state-file gates) — UTF-8 BOM prefix,
   duplicate keys (serde last-wins)
6. **Boundary** — empty string, single-character strings
7. **Override** (if applicable) — flag set, flag unset, flag with
   explicit `=false` or `=true` forms

For each variant, add a test case. The unit tests for the pure
gate helper cover most of these; the integration test (binary
spawn with prepared state) covers the ones that depend on
subprocess state (state-file reads, exit codes).

The discipline: write the variant list FIRST, then write the
tests from the list, then write the implementation. The goal is
to be boring — the gate passes every test on first implementation
because the test list already anticipated every bypass.

## How to Apply

When adding a new gate:

1. Write `normalize_gate_input` (or reuse an existing helper).
2. Encode the rule as a positive allowlist membership check over
   normalized inputs.
3. For state-file gates, implement fail-closed semantics for
   parse errors, wrong types, and missing fields.
4. In the plan, enumerate bypass variants explicitly in the
   Risks section.
5. Write the tests from the variant list, then the implementation.
6. Write a binary-level integration test that spawns the actual
   CLI with a prepared state file or CLI args — not just a unit
   test of the pure helper.

When reviewing an existing gate:

1. Grep for string comparisons in gate functions; confirm each
   comparison runs on normalized inputs.
2. Confirm the gate uses a positive allowlist for "permitted
   values" rather than a denylist for "forbidden values."
3. Confirm state-file reads fail CLOSED on parse errors and
   wrong types.
4. Confirm the binary-level integration test exists and covers
   the full decision matrix.
