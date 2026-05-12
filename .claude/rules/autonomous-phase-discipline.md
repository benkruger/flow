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
  (`stop_continue::check_autonomous_in_progress`) refuses a turn-end
  during an in-progress autonomous phase, so a model that "stops
  with text" gets blocked into continuing.

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
- When the user sends a message mid-phase, answer their message.
  That is different from pausing — the user initiated the
  interaction, so the autonomy contract is not violated.
- If context is genuinely exhausted, commit the current work with
  a message naming the task, then stop. The stop-continue hook
  logs the halt for the user to resume from. Do not pause at a
  point where nothing was committed.

## Scope

This rule applies to every phase that can be autonomous: Start,
Plan, Code, Review, Learn, Complete. The `continue: auto`
configuration is readable in every phase's `phase-enter`
response.

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

The Stop hook (`stop_continue::run()`) refuses a voluntary
turn-end with `{"decision":"block"}` when
`phases.<current_phase>.status == "in_progress"` AND
`skills.<current_phase>.continue == "auto"` (Simple `"auto"` and
Detailed `{"continue":"auto"}` shapes both recognized) AND
`_continue_pending` is empty. The block runs after
`check_first_stop` and `check_continue` so discussion mode and
multi-child-skill chains keep their semantics. The block reason
instructs user stop intent to route through `/flow:flow-abort`
or `/flow:flow-note`. PreToolUse hooks cannot observe a turn-end
with no tool call — only a Stop hook can — so this predicate
closes the text-only-stop hole that `validate-ask-user` cannot
reach.

## Prose-Based Pauses Bypass `AskUserQuestion`

The autonomous-mode discipline above forbids `AskUserQuestion`
prompts that the user did not ask for, and the
`validate-ask-user` hook enforces it mechanically — but only on
`AskUserQuestion` tool calls. A model that emits the question as
plain text and ends the turn without any tool call routes around
the AskUserQuestion gate entirely. Every prose-pause is the same
interruption shape, just expressed in a different surface.

The pattern looks like this: at a Code-phase task-entry boundary
in autonomous mode, instead of starting the TDD cycle, the model
writes 2-4 questions as a prose response and stops. No tool call
fires, so `validate-ask-user` does not see anything to block. The
existing autonomous-stop-refused predicate
(`stop_continue::check_autonomous_in_progress`) does refuse a
voluntary turn-end with no `_continue_pending` set, but its
generic block message points the model at `/flow:flow-abort` or
`/flow:flow-note` — guidance designed for "stop intent," not for
"resume execution." The targeted task-entry guard below gives a
more specific message that names the recovery rule.

### Failure modes

The pattern surfaces in three shapes, all forbidden:

- **Questions in plain text** at a task-entry boundary: "Should I
  proceed?", "Want me to also do Y?", "Confirm intent on Z?".
- **"Want me to..." / "Let me know..." / "Ready when you are"
  sign-offs** at task-entry boundaries — the same shape, just
  phrased as deferral instead of an explicit question.
- **Multi-option enumeration without an explicit user request** —
  "I could do A, B, or C" laid out as a menu when nothing in the
  conversation asked for choices.

### Mechanical enforcement

`stop_continue::run` runs a task-entry guard
(`check_prose_pause_at_task_entry`) BEFORE
`check_autonomous_in_progress`. The guard fires only when ALL of:

1. `phases.<current_phase>.status == "in_progress"`
2. `skills.<current_phase>.continue == "auto"`
3. `current_phase == "flow-code"` (Code phase scope only — Plan,
   Review, and Learn boundaries each have their own task
   shape and need separate analysis before this guard extends to
   them)
4. `code_task == 0` (no Code-phase task has been committed yet —
   the canonical task-entry boundary; later prose-pauses fall
   through to the broader `check_autonomous_in_progress` block)
5. `_continue_pending` is not set (mid-skill-chain pauses where
   the parent skill is awaiting a child completion are excluded)
6. The last assistant message in the persisted transcript
   contains a `?` outside fenced code blocks and inline code
   spans
7. The last assistant message was NOT followed by a tool_use
   block (Stop event received without any tool call)

When all seven hold, the hook refuses the Stop event with a
block message that cites both this rule and
`.claude/rules/autonomous-flow-self-recovery.md`, instructing the
model to classify the situation as mechanical (resume execution)
or substantive (call AskUserQuestion subject to validate-
ask-user, which the autonomous-phase block will then evaluate).
Pause shapes outside this guard's seven-condition window remain
advisory — the prose rule above is the primary instrument and
the targeted guard is the merge-conflict trip-wire for the
specific shape the conversation surfaces most often.

## Explicit User Pause Directives

The autonomous-mode block above protects against model-initiated
pauses — interruptions the user did not ask for. It does NOT
override **explicit user directives** to pause. When the user
types a pause instruction in plain English ("pause until I get
back", "stop here and wait", "pause after the next agent
returns"), the directive overrides the autonomous configuration
for the scope the user named. The model honors the pause at the
next natural boundary the user identified and stays halted until
the user explicitly says to continue.

The two surfaces are not in conflict. The autonomous-mode rule
forbids self-imposed pauses ("want me to continue?"). The user-
directive rule honors user-imposed pauses ("pause and wait"). The
distinguishing test: did the user type the pause instruction in
the conversation, or is the model imagining a pause point on its
own?

### Mechanical halt-pause contract

`stop_continue::check_halt_pending` is the predicate that detects
user pause directives during autonomous flows and emits a
conversation-preserving block message naming the closed
continue-token grammar. The predicate composes the new
transcript-walker helpers
(`transcript_walker::most_recent_user_message_since_skill_action`
and `transcript_walker::user_message_contains_continue_token`)
with the state-file fields `_halt_pending`, `_stop_instructed`,
and `_continue_pending` to track halt state across multiple Stop
events.

**Closed continue-token set.** The grammar that clears
`_halt_pending` is exactly: `continue`, `resume`, `proceed`,
`go ahead`, `keep going`. Comparison is case-insensitive with
word-boundary checks: `discontinue`, `resumed`, `proceedings`,
`goahead`, `keepgoing` do NOT clear the halt. Trailing
punctuation (`continue.`, `continue!`) is permitted because
non-word characters satisfy the trailing boundary. Two-word
tokens tolerate any run of ASCII whitespace between the words
(`go ahead`, `go  ahead`, `go\tahead`) but reject zero-whitespace
concatenations.

**Predicate ordering.** `check_halt_pending` runs FIRST in
`stop_continue::run`, before `check_first_stop`. This ordering
is load-bearing: if `check_first_stop` ran first when the user
typed "pause", the block would emit the discussion-mode message
("Wait for the user — they are not done talking"), which lacks
the do-not-advance guidance and the closed token grammar. The
halt-pause-specific message must dominate so the model holds
position correctly. The reverse ordering would also leak
`_stop_instructed=true` past the halt window, which compounds
into Bug 2 (residual halt residue carried into Complete).

**State-field lifecycle.**

- `_halt_pending: bool` — owned by `check_halt_pending`. Set to
  true when the user typed a non-continue-token message after
  the most recent assistant Skill action. Cleared when the user
  types a continue token. Default-false on missing or wrong-type
  values per `.claude/rules/state-files.md` Corruption Resilience.
- `_stop_instructed: bool` — owned by `check_first_stop` /
  `check_continue` (discussion-mode state). When
  `check_halt_pending` clears `_halt_pending` via a continue
  token, it also clears `_stop_instructed` in the same
  `mutate_state` call so the discussion-mode residue does not
  bleed forward into subsequent phases (closes Bug 2).
- `_continue_pending: string` — preserved across every set and
  clear of `_halt_pending`. The cascade's multi-child-skill
  resume path reads `_continue_pending` once the user clears the
  halt, so trampling it would break the resume contract.

**Persistence across multiple Stops.** When the user has typed a
non-continue-token message and `_halt_pending=true` is set, every
subsequent Stop event continues to block until a continue token
arrives. The persistence branch fires when the walker returns
`None` (no new user message since the most recent Skill action)
but `_halt_pending` was already true.

**Stale halt residue.** When the current phase is no longer
in-progress OR no longer configured `auto`, the predicate clears
a stale `_halt_pending=true` (a previous phase set it and that
phase has since completed). This prevents a halt set in Code
from bleeding forward into Complete.

**Preconditions for firing.** The predicate sets `_halt_pending`
only when:

1. The state file exists and parses as a JSON object.
2. `current_phase` is non-empty and present in `phases`.
3. `phases.<current_phase>.status == "in_progress"`.
4. `skills.<current_phase>.continue == "auto"` (Simple `"auto"`
   string or Detailed `{"continue":"auto"}` object — both
   recognized through `normalize_gate_input`).
5. `transcript_path` is non-empty AND passes
   `is_safe_transcript_path` validation.
6. The walker returns `Some(msg)` AND `msg` contains no continue
   token.

When any precondition fails, the predicate falls through to the
no-block path (after clearing stale `_halt_pending` per the
"Stale halt residue" branch).

**Fail-open.** Every error class returns no-block: missing state
file, unparseable JSON, missing or invalid transcript path,
walker `None`, missing `current_phase`. The Stop hook must never
panic; a hook crash terminates the user's session.

### Resumption discipline

When the user says "continue" or otherwise indicates resumption,
proceed from where the pause halted. Do not re-survey the
landscape, do not re-summarize what would be done, do not ask
"sure?" — the user has answered. The directive that ended the
pause is also the directive that re-authorizes the autonomous
configuration; the model is back in the same `continue: auto`
state it was in before the pause, and the same discipline
applies (no self-imposed pauses, commit at natural boundaries).

## User-Only Skill Carve-Out

The autonomous-phase block above protects against model-initiated
prompts. When a user types `/flow:flow-abort`, `/flow:flow-reset`,
`/flow:flow-release`, or `/flow:flow-prime` mid-flow, the
resulting skill invocation fires an `AskUserQuestion` for
destructive-operation confirmation — and that prompt is
user-initiated, not model-initiated, so it should fire even
during in-progress autonomous phases.

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
