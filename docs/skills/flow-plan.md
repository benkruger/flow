---
title: /flow-plan
nav_order: 18
parent: Skills
---

# /flow-plan

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-plan <topic>
```

Opens a structured planning conversation about a proposed change. The skill stays in discussion mode by default — surfacing clarifying questions, exploring the codebase, identifying risks — and dispatches to PM, Tech Lead, or CTO planning sub-agents only on explicit user request. When the user signals "ready" or "file it", the skill hands off to `/flow-create-issue` via the shared session conversation.

This is a thinking room, not a workflow. The skill never proposes direct edits, never commits, never files anything itself.

---

## What It Does

| Step | Name | Gate |
|------|------|------|
| 1 | Conversation Gate | HARD-GATE: topic argument required |
| 2 | Role Read | Reads `.flow.json` `role` field; absence-tolerant |
| 3 | Discussion Mode | HARD-GATE: no actions, no `AskUserQuestion`, no auto-dispatch |
| 4 | Persona Dispatch | HARD-GATE: render `## SCOPE REFUSAL` verbatim, no auto-escalation |
| 5 | Wrap-up | Clear marker, hand off to `/flow-create-issue` |

1. **Step 1 — Conversation Gate:** Verifies a topic argument was provided. Without a topic the skill has no anchor for the discussion; the gate clears the utility-in-progress marker and stops with usage guidance.
2. **Step 2 — Role Read:** Reads `.flow.json` for the optional `role` field — `"pm"`, `"tech-lead"`, `"founder-solo"`, or absent. Maps the role to a complementary-default suggestion (PM → Tech Lead voice, Tech Lead → PM voice, founder-solo → no preset). Treats absence and unknown values as "no preferred default" — never blocks on a missing field.
3. **Step 3 — Discussion Mode:** The default posture. Surfaces clarifying questions, explores the codebase via Read/Glob/Grep, identifies risks, iterates with the user. Never proposes actions, never files anything, never uses `AskUserQuestion`. Stays here until the user explicitly requests a persona dispatch (Step 4) or signals readiness to hand off (Step 5).
4. **Step 4 — Persona Dispatch:** On explicit user request ("PM view?", "Tech Lead view?", "CTO view?"), summarizes the discussion as `CONVERSATION_SUMMARY` + `PROPOSED_CHANGE` and invokes the named sub-agent (`flow:pm`, `flow:tech-lead`, or `flow:cto`) via the Skill tool. Renders the agent's response verbatim. When the response is a `## SCOPE REFUSAL` block, the HARD-GATE prohibits auto-escalation, soft-re-prompting, and personal performance of the refused analysis — the refusal surfaces as-is and the user chooses the next move.
5. **Step 5 — Wrap-up:** On a user readiness signal ("ready", "file it", "let's go"), clears the utility-in-progress marker, outputs the COMPLETE banner, and instructs the user to invoke `/flow-create-issue`. The planning context flows downstream via the shared session conversation — no scratch file, no state hand-off.

---

## Personas

Persona dispatch routes to one of three planning sub-agents — each with its own scope authority and escalation target.

| Persona | Skill identifier | Scope authority | Escalates to |
|---------|------------------|-----------------|--------------|
| PM | `flow:pm` | Copy, content, small changes with no new functionality or complexity | Tech Lead |
| Tech Lead | `flow:tech-lead` | Extensions of existing modules, new code following established patterns, refactors within current architecture, test additions | CTO |
| CTO | `flow:cto` | Novel architectural decisions, around-the-corner problems, outside-the-box alternatives | Terminus — no further escalation |

Each agent returns either an in-scope analysis or a `## SCOPE REFUSAL` block. The skill renders both verbatim. PM refuses overreach by naming Tech Lead as the next tier; Tech Lead refuses overreach by naming CTO; CTO is the terminus and produces no refusal block.

---

## Gates

- **Step 1 Conversation Gate** — `/flow:flow-plan` invoked without a topic argument clears the marker and stops with usage guidance. No interactive prompt; the user re-runs the command with `<topic>`.
- **Step 3 Discussion Mode HARD-GATE** — forbids direct edits, commits, issue filing, inline draft issue body composition, `AskUserQuestion` self-prompts, and auto-dispatch to a planning sub-agent on inferred scope. The skill stays conversational until the user signals a persona request or hand-off intent. The inline-draft-body prohibition is load-bearing: body composition happens downstream in `/flow-create-issue` where the include-bias scan runs before the draft is presented per `.claude/rules/include-bias-in-issues.md` — composing drafts inline during discussion bypasses that gate.
- **Step 4 Refusal Handling HARD-GATE** — when a sub-agent returns a `## SCOPE REFUSAL` block, the skill renders it verbatim and waits. Auto-escalation to the next tier, re-invoking the same agent with softer framing, and performing the refused analysis personally are all forbidden. The user chooses the next move (escalate, discuss, abandon).

---

## Hand-off

The skill produces no persistent artifact. Planning context flows downstream to `/flow-create-issue` via the shared session conversation: the next skill reads the same conversation history and synthesizes the captured discussion into the filed issue's Problem, Acceptance Criteria, Implementation Plan, Files to Investigate, and Context sections. The user types `/flow-create-issue` directly — `/flow-plan` never invokes it on the user's behalf.
