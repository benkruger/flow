---
name: flow-plan
description: "Open a structured planning conversation. Stays in discussion mode and dispatches to PM/Tech Lead/CTO sub-agents on explicit user request. Hands off to /flow:flow-create-issue when the user signals 'ready'. Usage: /flow:flow-plan <topic>"
---

# Flow Plan

Open a structured planning conversation about a proposed change. The
skill stays in discussion mode by default — surfacing clarifying
questions, exploring the codebase, identifying risks — and dispatches
to PM, Tech Lead, or CTO planning sub-agents only on explicit user
request. When the user signals "ready" or "file it", the skill hands
off to `/flow:flow-create-issue` via the shared session conversation.

This skill is a thinking room, not a workflow. It never proposes
direct edits, never commits, never files anything itself. Its only
job is to help the user reach a decision they trust, then hand the
decision to the issue-filing skill.

## Usage

```text
/flow:flow-plan <topic>
```

The `<topic>` argument names what the user wants to plan — a
behavior change, a refactor proposal, an architectural question, a
copy adjustment. The skill takes no other flags or arguments.

## Concurrency

This skill creates no shared state — no PRs, no issues, no labels,
no branch-scoped artifacts. The only side effect is the per-session
utility-in-progress marker (scoped to the user's Claude home, not
the project), which lets the Stop hook refuse turn-end while the
discussion-mode skill is running.

Multiple `/flow:flow-plan` sessions on the same machine in
different terminal windows are independent — each has its own
session id, its own marker, its own conversation context.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-plan — STARTING
──────────────────────────────────────────────────
```
````

Immediately after the banner, write the per-session "utility skill
in progress" marker so the Stop hook refuses turn-end while this
skill is running. Without the marker the model returns control to
the user when a planning sub-agent Skill tool returns mid-pipeline
at Step 4, breaking the unattended-discussion contract this skill
promises.

Rust resolves the active session_id at the CLI boundary by reading
the `CLAUDE_CODE_SESSION_ID` env var Claude Code supplies to every
Bash subprocess (Claude Code 2.1.132+); on older Claude Code
installs it falls back to the SessionStart capture file. The bash
invocation below passes `--skill` only; Rust supplies the
session_id itself.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-plan
```

If the marker-write call returns `status: error` with
`no session_id available` (no env var AND no capture file — rare,
only on Claude Code installs without per-subprocess env support and
without a SessionStart capture file), the skill proceeds without
the marker. The Stop hook treats a missing marker as a non-block,
so the skill runs without protection but does not break.

The marker is held across the entire discussion until the user
signals "ready" or "file it" (Step 5) or invokes Cancel from Step 1
when the topic argument is missing. Every skill-exit boundary
clears the marker so the Stop hook releases turn-end after the
skill completes.

On Claude Code installs without the per-subprocess env var, the
capture-file fallback resolves session_id independently at set and
clear time. A second Claude Code session whose SessionStart hook
overwrites the capture file between this skill's set and clear
calls can leave the marker orphaned at the original id. The same
orphan shape can occur when an unhandled error in Step 2, Step 3,
or Step 4 exits the skill before reaching a clear branch — the
marker persists and the Stop hook keeps refusing turn-end for the
rest of the session. Recovery in either case is
`rm ~/.claude/flow/utility-in-progress-*.json` after the skill
exits; the Stop hook treats a missing marker as a non-block.

---

## Step 1 — Conversation Gate

Verify that a topic argument was provided after the slash command.
The topic is what the planning conversation is about; without it
the skill has no anchor for the discussion.

<HARD-GATE>

If no topic argument was provided, clear the utility-in-progress
marker so the Stop hook does not refuse turn-end after the
rejection, then output the usage guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

> "Planning topic required. Usage: `/flow:flow-plan <topic>` where
> `<topic>` names what you want to discuss — a behavior change, a
> refactor proposal, an architectural question, or a copy
> adjustment."

Do not proceed to Step 2, propose direct edits, commit changes, or
take any action outside this skill without a topic argument.

</HARD-GATE>

---

## Step 2 — Role Read

Resolve the project root, then read `.flow.json` from it. The file
is gitignored and lives only at the main repo root — never in a
linked worktree — so the read must target the main repo path
regardless of whether the skill was invoked outside a worktree or
from inside one.

Run `git worktree list --porcelain`. The path on the first
`worktree` line is the main repo root. Read `<project_root>/.flow.json`
via the Read tool.

The file is written by `/flow:flow-prime` and stores per-user
preferences, including the optional `role` field that records the
user's primary working role.

Extract the `role` field from the JSON. The field is optional —
older `.flow.json` files written before the role-selection step
omit it. Treat absence, an unknown value, or a read failure as "no
preferred default" and proceed silently — never block on a missing
role.

Map the `role` value to a complementary planning default. The
default is informational only: the discussion-mode entry in Step 3
runs the same way for every role, and any sub-agent dispatch in
Step 4 still requires an explicit user request.

| `.flow.json` role value | Complementary default suggestion |
|---|---|
| `"pm"` | Suggest the Tech Lead voice as the default counterpart |
| `"tech-lead"` | Suggest the PM voice as the default counterpart |
| `"founder-solo"` | No preset — the user wears multiple hats |
| Absent / unknown / read failure | No preset |

If a complementary default applies, mention it once in the
conversation opener as a non-binding suggestion (for example, "Want
me to invite the Tech Lead voice on this?"). The user's answer is
not required to proceed to Step 3 — the suggestion is offered, then
the discussion runs regardless of how the user responds.

---

## Step 3 — Discussion Mode

The planning room exists so the user can think out loud against a
collaborator who reads the codebase and asks the right questions.
Discussion mode is the default posture — the skill stays here until
the user explicitly asks for a persona dispatch (Step 4) or signals
that the conversation is ready to hand off (Step 5).

In this step, the skill:

- Surfaces clarifying questions about the topic — what user-visible
  outcome is wanted, what constraints apply, what success looks
  like.
- Explores the codebase via the Read tool, the Glob tool, and the
  Grep tool to ground the conversation in current code. Cite file
  paths and line numbers when naming what exists today.
- Identifies risks, edge cases, prior approaches the user may not
  have considered, and architectural concerns that should be
  addressed before filing.
- Iterates with the user across multiple turns — challenging the
  proposed direction, summarizing what has been agreed, asking
  what comes next.

<HARD-GATE>

Discussion mode forbids action. While in this step, the skill must
NOT:

- Propose direct edits to source files, configuration, or rules —
  this is a thinking room, not a coding room.
- Commit changes or invoke `/flow:flow-commit`.
- File issues or invoke `/flow:flow-create-issue` — the hand-off
  belongs in Step 5 and is gated on an explicit user signal.
- Compose draft issue bodies inline. Discussion produces context
  for `/flow:flow-create-issue` via the shared session
  conversation, not artifacts. The skill must NOT compose
  `## Problem`, `## Acceptance Criteria`, `## Implementation
  Plan`, or any other issue-body sections during discussion —
  body composition happens in `flow-create-issue` from the
  conversation context, where the include-bias scan runs before
  the draft is presented per
  `.claude/rules/include-bias-in-issues.md`. Naming files, citing
  line numbers, and summarizing the agreed direction in
  conversational prose is fine; rendering a markdown-block draft
  with formal section headings is not.
- Use `AskUserQuestion` to manufacture a checkpoint the user did
  not ask for. The discussion is conversational; the user drives
  the cadence by sending messages. Per
  `.claude/rules/autonomous-phase-discipline.md`, never
  self-impose a pause via `AskUserQuestion`.
- Auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch (Step 4) requires the user to type the request
  in plain English.

Stay in discussion mode until the user types one of: a persona
request (then proceed to Step 4 for the named persona), a
hand-off signal ("ready", "file it", "let's go") (then proceed to
Step 5), or any other prose (then continue the conversation in
discussion mode).

</HARD-GATE>

---

## Step 4 — Persona Dispatch

When the user explicitly asks for a planning persona's view ("PM
view?", "What does Tech Lead think?", "CTO take?"), summarize the
discussion so far and dispatch to the named sub-agent. The skill
remains the orchestrator; the sub-agent returns a structured
analysis or a refusal block, and the skill renders the result.

### Summarize for the agent prompt

Build the agent prompt with two labeled sections:

- **CONVERSATION_SUMMARY** — a synthesis of the planning
  discussion so far. Name the topic, the constraints surfaced, the
  files explored, and the direction the user has indicated.
- **PROPOSED_CHANGE** — the concrete change being evaluated. Name
  the files that would be touched, the behavior change at issue,
  and the user-visible outcome.

### Invoke the named agent via the Skill tool

| User request | Sub-agent to invoke |
|---|---|
| PM view | `flow:pm` |
| Tech Lead view | `flow:tech-lead` |
| CTO view | `flow:cto` |

Pass the agent prompt above as the Skill tool's input.

<HARD-GATE>

When the sub-agent returns, render its response verbatim in the
conversation. If the response is a `## SCOPE REFUSAL` block, the
skill MUST surface it as-is and wait for explicit user direction.
The refusal is the agent's signal that the proposed change exceeds
its authority and a different tier should evaluate it.

When a `## SCOPE REFUSAL` block returns, the skill must NOT:

- Auto-escalate to the next tier (PM → Tech Lead, Tech Lead → CTO)
  without the user's explicit go-ahead. The escalation is the
  user's call, not the skill's.
- Re-invoke the same agent with softer framing, narrower scope, or
  any reworded prompt designed to elicit a non-refusal. The agent
  refused on principle, not on framing.
- Perform the refused analysis personally. The orchestrating model
  is not authorized to step into the role the agent declined; the
  refusal exists precisely because the change deserves a different
  level of judgment.

Present the refusal block to the user and ask them how to proceed.
If the user says "escalate to Tech Lead" or "ask the CTO", the
next persona dispatch fires in a fresh Step 4 invocation. If the
user wants to discuss the refusal in plain language, return to
Step 3.

</HARD-GATE>

When the agent returns an in-scope analysis (not a refusal), render
it verbatim and return to Step 3 so the user can react,
ask follow-ups, or request another persona's view. Do not assume
the agent's recommendation is the user's decision.

---

## Step 5 — Wrap-up

When the user signals readiness — "ready", "file it", "let's go",
"create the issue", or any equivalent phrasing — clear the
utility-in-progress marker, output the COMPLETE banner, and direct
the user to invoke `/flow:flow-create-issue`. The planning context
flows downstream via the shared session conversation; the
issue-filing skill reads the same conversation history and
synthesizes the captured discussion into the filed issue's
sections.

Clear the marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

Output the COMPLETE banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-plan — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then instruct the user:

> "The planning conversation is ready to hand off. Invoke
> `/flow:flow-create-issue` to capture the agreed direction as a
> pre-planned GitHub issue. The current conversation context carries
> the captured discussion forward — no scratch file, no state
> hand-off."

Do not invoke `/flow:flow-create-issue` yourself — the user types
the slash command directly, per the ask-first discipline in
`.claude/rules/flow-requires-user-initiative.md`.

---

## Hard Rules

- Never propose direct edits, commit changes, or file issues from
  inside this skill. The skill is a planning room; every artifact
  belongs to a downstream skill.
- Never present draft issue bodies inline. Discussion produces
  context for `/flow:flow-create-issue`, not artifacts. Body
  composition happens in `flow-create-issue` from the
  conversation context, where the include-bias scan runs before
  the draft is presented per
  `.claude/rules/include-bias-in-issues.md`.
- Never use `AskUserQuestion` during discussion mode. The
  discussion is conversational; the user drives the cadence.
- Never auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch requires the user to type the request in plain
  English ("PM view?", "Tech Lead view?", "CTO view?").
- Never auto-escalate, re-invoke with softer framing, or perform
  the refused analysis personally when a sub-agent returns a
  `## SCOPE REFUSAL` block. Render the refusal verbatim and wait
  for explicit user direction on the next move.
- Never write to FLOW per-branch state surfaces. The skill is
  stateless beyond the per-session utility-in-progress marker
  scoped to the user's Claude home — planning context flows to
  `/flow:flow-create-issue` via the shared session conversation,
  not via persisted artifacts.
- Treat absence or unknown values of the `.flow.json` `role` field
  as "no preferred default" and proceed silently. Never block on a
  missing field.
- Never use Bash to print banners — output them as text in your
  response.
- All `bin/flow` calls use `${CLAUDE_PLUGIN_ROOT}/bin/flow` — bare
  `bin/flow` only resolves inside the FLOW repo itself, not in
  target projects.
