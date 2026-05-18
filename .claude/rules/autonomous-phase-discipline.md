# Autonomous Phase Discipline

When a phase is configured for autonomous execution (`continue: auto`
in the state file's skills section, typically propagated from the
`--auto` flag), the session must not introduce user-facing pauses
that the user did not request.

## The Rule

During any phase with `continue: auto`:

- Never emit `AskUserQuestion` for checkpoints the user did not ask
  for — "want me to proceed?", "want me to continue?", "should I
  pause for context?" are all prohibited.
- Never self-declare a "context check", "budget check", or "session
  hand-off" mid-phase. The stop-continue hook is the only
  permissible signal for external help.
- Never mark state counters (like `code_task`) as complete and then
  halt without committing the corresponding work. The counter and
  the commit must advance together.
- Never unilaterally decide the flow is "too big" and ask whether
  to continue — autonomy means the user already answered that
  question when they chose `--auto`.
- Never end the turn voluntarily without producing a tool call.
  When context is exhausted, commit the in-flight work at a natural
  boundary; the Stop-hook predicate
  (`stop_continue::check_autonomous_stop`) refuses a turn-end during
  an in-progress autonomous phase, so a model that "stops with
  text" gets blocked into continuing.

If Claude feels the urge to pause because of context pressure, a
long-running task, or uncertainty about scope: commit the in-flight
work at a natural boundary, then resume on the next task. Pausing
to ask the user is an interruption; committing and continuing is
not.

## Why

Autonomous flows are explicitly configured by the user. A
self-imposed pause defeats the configuration — the user has to
intervene to say "please continue the thing I already told you to
continue." Every such intervention costs trust and round-trip
latency.

## How to Apply

- At every step boundary in a `continue: auto` phase, the next
  action is either (a) the next skill instruction or (b) a
  self-invocation via Skill tool. Never an `AskUserQuestion` that
  is not already mandated by the skill.
- If the skill's HARD-GATE says to ask the user, follow the gate.
  If the skill does not instruct a pause, do not invent one.
- When the user sends a message mid-phase, the Stop hook sets
  `_halt_pending=true` and allows the Stop so the model can answer.
  Every subsequent Stop event then blocks until the user invokes
  `/flow:flow-continue` (resume) or `/flow:flow-abort` (close the
  flow). See "The Two-Exit Halt Model" below.
- If context is genuinely exhausted, commit the current work with
  a message naming the task, then stop. The stop-continue hook
  logs the halt for the user to resume from. Do not pause at a
  point where nothing was committed.

## Scope

This rule applies to every phase that can be autonomous: Start,
Plan, Code, Review, Learn, Complete. The `continue: auto`
configuration is readable in every phase's `phase-enter`
response.

## System-Initiated Prompts

The model-initiated-pauses rule above forbids prompts the model
invents. The same principle extends to prompts the SYSTEM raises
on the model's behalf: when a phase is configured `continue:
auto`, no permission prompt may reach the user regardless of
which subsystem raises it. Claude Code platform protections,
`FLOW_DENY` patterns, `UNIVERSAL_ALLOW` misses, sensitive-path
heuristics, and any other gate that could surface a prompt are
all in scope. The user authorized continuous execution by
choosing autonomous mode; a permission prompt that requires the
user to type "yes" defeats that authorization just as completely
as a model-initiated `AskUserQuestion` would.

System-initiated prompts have two response shapes. The right
shape depends on whether the operation that triggered the prompt
is legitimate:

- **Legitimate operation reaches a sanctioned-tool gap.** The
  model is doing exactly what the skill instructs, but the
  sanctioned tool surface does not cover the underlying need —
  a directory creation that the Write tool cannot perform under
  `.claude/`, a state-derived read that the allow list does not
  cover, an artifact path the permission model does not anticipate.
  The fix is to extend the sanctioned-tool surface so the
  operation reaches the user without a prompt: route the write
  through `bin/flow write-rule` (which creates parent directories
  internally), add the missing `UNIVERSAL_ALLOW` entry, or build
  a new subcommand the skill can call. Adding to the sanctioned
  surface is permanent; the next session inherits the fix.
- **Model reaches for an unsanctioned operation.** The model
  invents a shape the sanctioned surface deliberately excludes —
  reading the persisted transcript JSONL for context recovery,
  writing a placeholder file to anchor a later redirect, calling
  an interpreter eval form to batch operations. The fix is to
  remove the unsanctioned operation at source: rewrite the skill
  or agent to reach for the documented sanctioned alternative
  (`compact_summary` in `.flow-states/<branch>/state.json`, the
  Read tool against persisted command output, sequential Bash
  calls), and add a mechanical guard so the unsanctioned shape
  cannot reappear.

Choosing which shape applies is a design decision, not a
runtime classification. A prompt that surfaces during a flow is
a signal that the design upstream of the prompt is incomplete —
either the sanctioned surface needs extending, or the
unsanctioned operation needs removing at source.

Cross-references:

- `.claude/rules/permissions.md` — the deny-list and allow-list
  discipline that governs which Bash invocations and tool calls
  fall through cleanly under autonomous mode.
- `.claude/rules/skill-authoring.md` "Platform Constraints" —
  the carve-out for paths Claude Code protects regardless of
  settings, including the `bin/flow write-rule` redirect for
  `.claude/` writes.
- `.claude/rules/post-compaction-recovery.md` — the sanctioned
  recovery surface (`compact_summary` in the state file)
  replacing the unsanctioned transcript JSONL read.
- `.claude/rules/no-placeholder-anchors.md` — the rule that
  forbids placeholder-file-then-redirect anchoring as an
  unsanctioned operation, regardless of destination.

## Enforcement

The prose rule above is backed by two mechanical hooks. The first
gates `AskUserQuestion`; the second gates the Stop event itself.

The `validate-ask-user` hook
(`src/hooks/validate_ask_user.rs::validate()`) refuses
`AskUserQuestion` tool calls with exit 2 when the state file
records BOTH `phases.<current_phase>.status == "in_progress"` AND
`skills.<current_phase>.continue == "auto"`. Two skill-config
shapes are recognized: the bare string form
(`skills.<phase> = "auto"`) and the object form
(`skills.<phase> = {"continue": "auto", ...}`) — corresponding to
`SkillConfig::Simple` and `SkillConfig::Detailed` in
`src/state.rs`.

The `phases.<current_phase>.status` check is intentional. After
`phase_complete()` writes `current_phase = <next-phase>` the
next phase's status is still `"pending"` until `phase_enter()`
sets it to `"in_progress"`. Scoping the block to `"in_progress"`
keeps the transition-boundary window open so the completing
skill's HARD-GATE can fire `AskUserQuestion` to approve the
transition (e.g., in mixed-mode flows where Code is manual and
Review is auto). Without this scope, the approval prompt
would be blocked and the flow would deadlock.

Ordering inside the hook: the block path runs before the
pre-existing `_auto_continue` auto-answer path. When the current
phase is `in_progress` and `auto`, the block wins even if
`_auto_continue` is set — the user's explicit per-skill
`continue=auto` configuration takes priority over the transient
transition-boundary safety net. Outside that in-progress+auto
window, `_auto_continue` behaves unchanged.

The blocked tool call returns the rejection message to the
model via stderr so the session adapts instead of stalling.

The Stop hook (`stop_continue::check_autonomous_stop`) refuses a
voluntary turn-end with `{"decision":"block"}` when
`phases.<current_phase>.status == "in_progress"` AND
`skills.<current_phase>.continue == "auto"` (Simple `"auto"` and
Detailed `{"continue":"auto"}` shapes both recognized). The
predicate composes three rules — see "The Two-Exit Halt Model"
below — that together close the text-only-stop hole that
`validate-ask-user` cannot reach: PreToolUse hooks observe only
tool calls, but the Stop hook fires on the Stop event itself, so a
model that ends the turn with prose alone is still refused.

## The Two-Exit Halt Model

The autonomous-mode block above protects against model-initiated
pauses — interruptions the user did not ask for. The halt model
below defines how the model handles real user messages mid-flow:
the message is acknowledged with one Stop, and every subsequent
Stop is refused until the user explicitly resumes or aborts.

The two sanctioned exits are:

- `/flow:flow-continue` — clears `_halt_pending` and resumes the
  autonomous flow. The user types this when they want the flow to
  proceed past the pause.
- `/flow:flow-abort` — closes the PR, deletes the worktree, and
  removes the state file. The user types this when they want to
  abandon the flow.

Both exits are user-only skills (see
`.claude/rules/user-only-skills.md`). The model cannot invoke
them — Layer 1's `validate-skill` hook blocks any Skill tool call
naming them unless the most recent user turn typed the matching
slash command.

### Mechanical halt-pause contract

`stop_continue::check_autonomous_stop` is the unified predicate
that owns the halt window. It composes
`transcript_walker::most_recent_user_message_since_skill_action`
with the state-file field `_halt_pending` to track halt state
across multiple Stop events.

**Three rules.** The predicate's behavior depends on whether a
conversational-prose user message appeared since the model's
most recent Skill action AND whether `_halt_pending` is already
set:

- **Rule 1 — no halt, no new user message.** Refuse the Stop
  with the encouraging message
  `"Stop Refused: Continue, you can do it. Don't give up, you
  got this! No excuses!"`. The autonomous flow must keep going —
  `continue: auto` already authorized continuous execution.
- **Rule 2 — halt pending, no new user message.** Refuse the
  Stop with a message naming the two exits: `/flow:flow-continue`
  to resume, `/flow:flow-abort` to close the flow. The block
  persists across every subsequent Stop until `_halt_pending` is
  cleared.
- **Conversation pass-through.** When a real **conversational
  prose** user turn appears since the most recent Skill action,
  set `_halt_pending=true` and ALLOW the Stop so the model can
  answer. On the next Stop without a new conversational prose
  message, Rule 2 fires. Imperative slash-command turns
  (`<command-name>/<skill></command-name>` or the two-line
  `<command-message>...</command-message>` shape) are filtered
  by the walker and do NOT trigger pass-through — they neither
  set `_halt_pending` nor authorize a voluntary stop. Within
  that filter, `/flow:flow-continue` is the universal resume
  directive: the walker additionally watermarks preceding prose
  to `None`, so a user who first paused with prose and then
  typed `/flow:flow-continue` sees the next Stop fire Rule 1
  (encouraging refusal) instead of re-arming Rule 2. The
  resulting two-channel UX: prose triggers pass-through and
  conversation; `/flow:flow-continue` resumes without
  authorizing voluntary stops regardless of whether the halt
  was caused by user prose or by an external interrupt. The
  imperative-vs-conversational discriminator and the
  `/flow:flow-continue` watermark live in
  `.claude/rules/transcript-shape.md` "Real User Turns:
  Imperative vs Conversational Shapes".

**Who clears `_halt_pending`.** The flag is cleared by exactly
two writers:

- `bin/flow clear-halt` — invoked by `/flow:flow-continue`. The
  subcommand self-gates on the persisted transcript: it refuses
  unless the most recent user turn's `message.content` STARTS
  with either of the two emission shapes Claude Code uses for
  the slash command — the two-line
  `<command-message>flow:flow-continue</command-message>\n<command-name>/flow:flow-continue</command-name>`
  (Claude Code 2.1.140+) or the legacy
  `<command-name>/flow:flow-continue</command-name>`. The walker
  accepts either via `starts_with` disjunction.
- `check_autonomous_stop` itself — when the current phase is no
  longer in-progress OR no longer configured `auto`. This
  prevents a halt set in one phase from bleeding forward into
  the next phase. The clearing also happens in `phase_enter` as
  belt-and-suspenders defense per
  `src/phase_transition.rs::phase_enter`.

**State-field lifecycle.**

- `_halt_pending: bool` — owned by `check_autonomous_stop` and
  cleared by `bin/flow clear-halt`. Set to `true` when the user
  typed a real message after the most recent assistant Skill
  action. Default-false on missing or wrong-type values per
  `.claude/rules/state-files.md` "Corruption Resilience".
- `_continue_pending: string` — preserved across every set and
  clear of `_halt_pending`. The cascade's multi-child-skill
  resume path reads `_continue_pending` once the halt is
  cleared, so trampling it would break the resume contract.

**Synthetic-turn discrimination.** The walker filters out
synthetic user turns (tool_result wrappers, hook-injected
feedback turns carrying `isMeta:true`) via
`transcript_walker::is_real_user_turn` per
`.claude/rules/transcript-shape.md`. Without that filter, a
Stop-hook refusal turn would be misclassified as a real user
message and set `_halt_pending` spuriously.

**Persistence across multiple Stops.** When the user has typed a
non-continue message and `_halt_pending=true` is set, every
subsequent Stop event continues to block (Rule 2) until
`/flow:flow-continue` clears the flag. The persistence branch
fires when the walker returns `None` (no new user message since
the most recent Skill action) but `_halt_pending` was already
true.

**Fail-open.** Every error class returns no-block: missing state
file, unparseable JSON, missing or invalid transcript path,
walker `None`, missing `current_phase`. The Stop hook must never
panic; a hook crash terminates the user's session.

### Defense in depth — halt gates on Skill and Bash

`check_autonomous_stop` blocks the Stop event itself, but
`_halt_pending=true` also gates two PreToolUse surfaces so the
model cannot route around the halt by invoking Skills or
Bash commands during the halt window:

- **`validate-skill` halt gate**
  (`src/hooks/validate_skill.rs::validate` Layer 2). Blocks any
  Skill tool call during halt unless the skill is in
  `USER_ONLY_SKILLS` AND the most recent user turn typed the
  matching slash command. The user-only exits
  (`/flow:flow-continue`, `/flow:flow-abort`) pass through.
- **`validate-pretool` halt gate**
  (`src/hooks/validate_pretool.rs` after Layer 9). Blocks the
  closed allowlist of flow-advancing Bash commands during halt:
  `bin/flow phase-enter`, `phase-finalize`, `phase-transition`,
  `finalize-commit`, `set-utility-in-progress`, and
  `set-timestamp --set code_task=*`. `bin/flow clear-halt` is
  not in the advancing set and falls through cleanly — its own
  self-gate (Layer 1 of `validate-skill` plus the
  transcript-walker check inside `clear-halt::run_impl`) is the
  load-bearing protection against bypass.

Together, the Stop hook and the two PreToolUse halt gates form
the three-surface defense that closes every model-initiated
action during the halt window.

### Resumption discipline

When the user invokes `/flow:flow-continue`, proceed from where
the halt landed. Do not re-survey the landscape, do not
re-summarize what would be done, do not ask "sure?" — the user
has answered. The slash-command invocation is the directive that
re-authorizes the autonomous configuration; the model is back in
the same `continue: auto` state it was in before the halt, and
the same discipline applies (no self-imposed pauses, commit at
natural boundaries).

## User-Only Skill Carve-Out

The autonomous-phase block above protects against model-initiated
prompts. When a user types `/flow:flow-abort`, `/flow:flow-reset`,
`/flow-release`, `/flow:flow-prime`, or `/flow:flow-continue`
mid-flow, the resulting skill invocation
may fire an `AskUserQuestion` (destructive confirmation, version
bump confirmation, etc.) — and that prompt is user-initiated, not
model-initiated, so it should fire even during in-progress
autonomous phases.

`validate-ask-user::user_only_skill_carve_out_applies` recognizes
this case and allows the AskUserQuestion through. The check
inspects the persisted transcript: when the most recent assistant
Skill tool_use call (since the most recent user turn) targets a
skill in `crate::hooks::transcript_walker::USER_ONLY_SKILLS`, the
prompt fires. The presence of an assistant Skill call to a user-
only skill is the user-direction signal — `validate-skill` Layer
1 ensures the model can only reach that Skill call after the user
typed the slash command. See `.claude/rules/user-only-skills.md`
Layer 2 for the full design.

## Shared-Config Carve-Out

The autonomous-phase block above protects against
model-initiated prompts. The shared-config block from
`validate_worktree_paths` (see `.claude/rules/permissions.md`
"Shared Config Files — Express User Permission Required") is the
opposite shape: another hook explicitly instructs the model to
call `AskUserQuestion` to confirm a shared-config edit. Without a
carve-out, the autonomous-phase block refuses the very prompt the
prior hook demanded — the flow deadlocks while two hooks
contradict each other.

The trigger is system-initiated, not model-initiated: the
shared-config BLOCKED message itself directs the next action.
Letting the prompt fire completes the confirmation flow the
system asked for.

`validate-ask-user`'s `run_impl_main` calls
`crate::hooks::transcript_walker::recent_edit_blocked_on_shared_config`
between the user-only-skill carve-out and the block return. The
helper walks the persisted transcript backward from the file
tail, capped at `SHARED_CONFIG_BLOCK_BYTE_CAP` (4 MB), and
returns `true` when it finds a `tool_result` block with
`is_error: true` whose `content` contains the literal substring
`"is a shared configuration file"` since the most recent real
user turn. The substring is uniquely emitted by
`crate::hooks::validate_worktree_paths::validate_shared_config`
and locked in place by a presence-contract test in
`tests/hooks/validate_worktree_paths.rs`.

The user-only carve-out is checked first; both produce the same
allow outcome, so the order is semantically irrelevant but the
ordering is locked by an explicit regression test
(`both_carve_outs_can_apply_user_only_wins_first`). Older user
turns and tool_results predating the most recent real user turn
are invisible to the helper — only the active confirmation
window matters.
