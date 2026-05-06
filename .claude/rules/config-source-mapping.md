# Config Source Mapping

When a plan task modifies a config file (`.claude/settings.json`,
`.flow.json`, `flow-phases.json`, `hooks/hooks.json`, etc.) or claims
that a derived value (a hash, a checksum, a generated artifact) is
affected by such a modification, the plan must cite the specific
Rust code, hook script, or downstream consumer that reads the
value being changed. Wrong assumptions about config-source
mapping produce plans that propose work the consumer never
performs (e.g., a hash bump for a value the hash function does
not read) and confuse Code-phase review when the proposed effect
fails to materialize.

## Why

A plan that says "removing X from `.claude/settings.json` will
force a `compute_config_hash` change" is a load-bearing claim
about the system's data flow. If the hash function actually reads
Rust constants in `src/prime_check.rs::UNIVERSAL_ALLOW`/
`FLOW_DENY` and not the JSON file at all, the plan's task list
is built on a false premise: the hash bump never happens, and
every downstream task that depends on it (e.g., updating a pinned
hash literal in tests, signaling a re-prime requirement) is
either redundant or wrong.

The fix is upstream of Code phase: the plan must verify which
code reads the modified value before making downstream claims.
The verification step is cheap (one grep, one Read of the
identified consumer) and catches the assumption while it is
still in plan prose, where it costs minutes to fix instead of
hours of Code-phase rework.

## The Rule

For every plan task that modifies a config file, the plan's
Tasks or Risks section must enumerate:

1. **The config surface modified.** Exact file path and the
   specific entry, key, or field being added, removed, or
   changed.
2. **The consumer(s).** Every Rust function, hook script,
   subcommand, or runtime read that consults this value at
   runtime. For each consumer, cite the file and line range
   (or function name) where the read occurs.
3. **The downstream effects claim.** If the plan asserts that
   a downstream value (hash, sentinel, cached artifact,
   computed JSON field) changes as a consequence, name the
   computation and verify by reading its implementation that
   the modified value is in the input set.
4. **The verification path.** A grep, Read, or bin/flow
   subcommand invocation that confirms the consumer reads the
   value as claimed. Place the verification artifact in the
   plan's Risks or Exploration section so Code phase can
   re-run it.

## Canonical Config-Source Mappings

These are the load-bearing config-to-reader mappings in the
FLOW codebase as of this rule's authoring. New mappings
discovered during plan exploration should be added here so
future plan authors can cite an authoritative reference.

### `compute_config_hash`

Reads ONLY the Rust constants in `src/prime_check.rs`:
- `UNIVERSAL_ALLOW` — allow-list patterns
- `FLOW_DENY` — deny-list patterns
- `EXCLUDE_ENTRIES` — `.git/info/exclude` patterns

Does **not** read `.claude/settings.json` directly. A change to
the JSON file affects the hash only if the same change is
reflected in the matching Rust constant.

Pinned hash consumer: `tests/prime_check.rs::compute_config_hash_uses_python_default_formatter`.

### `.claude/settings.json`

Read by Claude Code at runtime — never by Rust code. Permission
prompts honor the allow/deny lists, and the global
`validate-pretool` hook enforces the merged allow list during
active flows. No Rust subcommand parses this file.

Modifications take effect immediately for the active session.
There is no compile-time consumer that needs re-running on
change.

### `.flow.json`

Read at flow-start by `start-init`/`init_state` and copied into
the per-flow state file. After `flow-start`, the running flow
reads its preferences (`skills`, `commit_format`, etc.) from
`.flow-states/<branch>/state.json`, never from `.flow.json`
directly. Code, Code Review, Learn, and Complete phases
operate from the state file copy.

Hash inputs: `config_hash` and `setup_hash` are stored in
`.flow.json` and compared by `prime_check.rs` when version
changes. The hash values are derived from Rust source (see
above), not from `.flow.json` itself.

### `flow-phases.json`

Read at runtime by `bin/flow check-phase`, `phase-enter`,
`phase-finalize`, and `phase-transition`. Defines the phase
state machine: phase names, commands, valid back-transitions.
A change to phase ordering or back-transitions takes effect on
the next subcommand invocation.

### `hooks/hooks.json`

Read by Claude Code at session start. Defines hook
registration: which Rust subcommand handles each tool-use
event. A change requires a new Claude Code session to take
effect (Claude Code caches the hook map).

### `assets/bin-stubs/<tool>.sh`

Read by `prime_setup.rs` when installing stubs into target
projects. The `# FLOW-STUB-UNCONFIGURED` marker is read by
`ci.rs::any_tool_is_stub` to decide whether to write the CI
sentinel. Stub content changes affect only NEW prime
installations; pre-existing user scripts are never overwritten.

## How to Apply

**Plan phase.** When drafting a plan task that touches any
config file:

1. Identify the modification site (path + entry).
2. Run the verification grep for the modified value:
   ```text
   grep -rn "<modified-value>" src/ hooks/ skills/ .claude/
   ```
3. Read the matched files to confirm which consumer actually
   uses the value at runtime.
4. Add a row to the plan's Risks or Exploration section:
   `<modified-value> in <file> read by <consumer>; downstream
   effect: <claim verified by reading <consumer-impl>>`.
5. If the plan asserts a hash bump or other derived effect,
   cite the computation function and verify the modified value
   is in its input set.

**Code phase.** When discovery during implementation reveals
the plan's mapping was wrong, log the deviation per
`.claude/rules/plan-commit-atomicity.md` "Plan Signature
Deviations Must Be Logged" and update the plan or the canonical
mapping table above.

**Code Review phase.** The reviewer agent checks every
config-modification task in the diff against the plan's cited
consumers. A modification with no cited consumer or with a
consumer that does not actually read the modified value is a
Real finding fixed in Step 4 by either adjusting the plan's
claim or removing the misguided downstream task.

## Cross-References

- `.claude/rules/plan-commit-atomicity.md` "Plan Signature
  Deviations Must Be Logged" — the mechanism for recording
  Code-phase discoveries that contradict plan-time
  assumptions.
- `.claude/rules/external-input-validation.md` — sibling rule
  for runtime input validation. This rule is about plan-time
  data-flow assumptions; that rule is about runtime input
  trust.
- `src/prime_check.rs` — the canonical home of the
  `UNIVERSAL_ALLOW`/`FLOW_DENY`/`EXCLUDE_ENTRIES` constants
  and their hash function.
