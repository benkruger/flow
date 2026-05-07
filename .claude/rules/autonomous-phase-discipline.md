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
Plan, Code, Code Review, Learn, Complete. The `continue: auto`
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
Code Review is auto). Without this scope, the approval prompt
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
