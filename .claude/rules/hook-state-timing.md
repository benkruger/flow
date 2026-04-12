# Hook State Read Timing

When a PreToolUse hook (or any hook that runs mid-session) reads
fields from the FLOW state file, trace the read window against the
write path before designing the gate. State fields that are written
by phase-transition commands — `current_phase`, `phases.<N>.status`,
`_auto_continue`, `_continue_pending` — are mutated at specific
moments in the phase lifecycle, and hooks that consume those fields
must know when those mutations land relative to when the hook fires.

## Why

The initial `validate-ask-user` block path (PR #1053) gated on
`skills.<current_phase>.continue == "auto"` without accounting for
WHEN `current_phase` is written. `phase_complete()` advances
`current_phase` to the NEXT phase before the completing skill's
HARD-GATE fires `AskUserQuestion`. With a manual→auto transition
(Code=manual with Code Review=auto in the Recommended preset), the
skill fires the approval prompt, the hook reads `current_phase` =
the next (auto) phase, and blocks the approval — flow deadlocked.

The fix was to add a phase-status predicate
(`phases.<current_phase>.status == "in_progress"`) so the block
only fires when the phase is actively executing. The bug was
catchable at Plan time if the Risks section had enumerated state
reads against state writes; it wasn't, so Code Review's pre-mortem
agent caught it post-implementation. Escalating the rule from
instruction-level to hook-level is a class of change where this
pattern is most likely to recur.

## The Rule

When planning a hook or any mid-session code path that reads state
written by a phase-transition command, the Plan phase **Risks**
section must enumerate:

1. **The fields the hook reads.** Every `state.get("...")` call in
   the hook body.
2. **The writers of each field.** Which Rust subcommand(s) write
   each field — `phase_complete`, `phase_enter`, `phase_finalize`,
   `set_timestamp`, `init_state`, etc. Grep `src/` for
   `state["<field>"] = ` to find every writer.
3. **The temporal ordering.** For each field, the order of writes
   relative to observable skill actions. Example: `current_phase`
   is advanced by `phase_complete()` BEFORE the skill's HARD-GATE
   fires.
4. **The read window.** When is the hook invoked relative to those
   writes? Does the hook assume one field is stable while another
   is in-flight?

## How to Apply

**Plan phase.** For every hook that reads state, add a risk in the
plan's Risks section that lists the fields (1), the writers (2),
the ordering (3), and the read window (4). If the answer to (4) is
"the hook can fire between any two state mutations," identify an
immutable marker (like `phases.<N>.status == "in_progress"`) that
captures the invariant the hook actually needs and gate on that
instead of on the advancing field directly.

**Code phase.** In the hook's inline comments, document the
in-flight state the hook tolerates. State the invariant the hook
depends on and name the guard field that enforces it. Do not rely
on implicit ordering of state mutations — an implicit dependency
is a latent bug that surfaces the first time a new phase-transition
path is added.

**Code Review phase.** The pre-mortem agent should explicitly check
"can this hook fire in a state where `current_phase` has advanced
but the new phase hasn't been entered yet?" as part of its
state-consistency review. See `agents/pre-mortem.md` for the
Premise → Trace → Conclude template.

## Pattern Examples

**Good** — scoped to a stable invariant:

```rust
// Block only when the phase is actively executing. `in_progress`
// is set by phase_enter() and cleared by phase_finalize(), so the
// transition-boundary window (between phase_complete and
// phase_enter of the next phase) is correctly excluded.
if phase_status == "in_progress" && is_auto {
    return block_response();
}
```

**Bad** — relies on an advancing field without a stability guard:

```rust
// current_phase is advanced by phase_complete() before the
// completing skill's HARD-GATE fires. Reading it here without a
// status guard means the hook blocks the next phase's config
// while the previous phase is still firing its approval prompt.
if skills[current_phase].continue == "auto" {
    return block_response();
}
```

## Enumeration Reference

The canonical state-field writers in the FLOW Rust tree are:

- `src/phase_transition.rs` — `phase_complete` advances
  `current_phase`, writes `_auto_continue` and `_continue_pending`,
  sets `phases.<N>.status` on the completing phase to `"complete"`;
  `phase_enter` sets `phases.<N>.status` of the new phase to
  `"in_progress"`.
- `src/phase_enter.rs` — the phase-entry path; clears
  `_auto_continue`, `_continue_pending`, `_continue_context`.
- `src/phase_finalize.rs` — phase completion via `phase-finalize`;
  shares state mutations with `phase_transition::phase_complete`.
- `src/commands/init_state.rs` — initial state file creation at
  flow-start.
- `src/commands/set_blocked.rs` / `src/hooks/validate_ask_user.rs`
  `set_blocked` helper — writes `_blocked` timestamp.

When a hook reads any field mutated by the above, the read window
analysis is mandatory.
