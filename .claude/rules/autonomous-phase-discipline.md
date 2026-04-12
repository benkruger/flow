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

The specific failure mode this rule targets: a long Code phase
(many tasks, many commits) where the session begins to feel
context pressure and injects a pause to "check in" with the user.
The user then has to say "you stopped for no reason, keep going."
This happened during PR #1046 twice in the same Code phase; see
the state file's `findings[]` for the recorded learnings.

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

The prose rule above is backed by a mechanical PreToolUse hook.
The `validate-ask-user` hook
(`src/hooks/validate_ask_user.rs::validate()`) refuses
`AskUserQuestion` tool calls with exit 2 when the state file
records `skills.<current_phase>.continue == "auto"` — both the
Simple form (`skills.<phase> = "auto"`) and the Detailed form
(`skills.<phase> = {"continue": "auto", ...}`) are recognized.
The block path precedes the existing `_auto_continue` auto-answer
path in the same hook, so the user's explicit per-skill
continue=auto config wins over any transient transition-boundary
state. The blocked tool call returns the rejection message to the
model via stderr so the session adapts instead of stalling.

This is the hook-level escalation described by
`.claude/rules/hook-vs-instruction.md` — the rule exists because
the user-visible pauses observed during PR #1046 proved that
instruction-level enforcement alone was insufficient.
