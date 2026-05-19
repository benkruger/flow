---
title: /flow-explore
nav_order: 21
parent: Skills
---

# /flow-explore

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-explore <topic>
```

Opens a problem-statement conversation about something the user wants to build, fix, or change. The skill stays in discussion mode by default — surfacing clarifying questions, exploring prior issues, identifying the user-visible outcome — and dispatches to PM, Tech Lead, or CTO planning sub-agents only on explicit user request. When the user signals "ready" or "file it", the skill captures the agreed problem statement as a vanilla GitHub issue (`## What`, `## Why`, `## Acceptance Criteria`) without an Implementation Plan.

The output is a problem statement, not a design. Implementation decomposition belongs in `/flow:flow-plan #N`, which a Tech Lead runs against the issue this skill files.

---

## What It Does

| Step | Name | Gate |
|------|------|------|
| 1 | Conversation Gate | HARD-GATE: topic argument required |
| 2 | Role Read | Reads `.flow.json` `role` field; PM is the default voice |
| 3 | Discussion Mode | HARD-GATE: no `decompose:decompose`, no sentinels, no Implementation Plan; source reads permitted with composition discipline |
| 4 | Persona Dispatch | HARD-GATE: render `## SCOPE REFUSAL` verbatim, no auto-escalation |
| 5 | Wrap-up | Capture What/Why/Acceptance, validate with `--mode vanilla`, file with `--label vanilla` directly on the user's readiness signal |

1. **Step 1 — Conversation Gate:** Verifies a topic argument was provided. Without a topic the skill has no anchor for the discussion; the gate stops with usage guidance.
2. **Step 2 — Role Read:** Reads `.flow.json` for the optional `role` field. PM is the default voice for `/flow:flow-explore` regardless of role; the role only adjusts a one-line conversational note.
3. **Step 3 — Discussion Mode:** The default posture. Surfaces clarifying questions, reads prior GitHub issues via `gh issue view`, identifies acceptance criteria, iterates with the user. Reading source files via the Read tool, grepping, and globbing is permitted so claims can be grounded in observed behavior; the boundary the skill enforces is composition discipline (translate code-grounded findings into user-visible prose when capturing). Forbids invoking `decompose:decompose`, writing FLOW-PLAN sentinel markers, and composing an `## Implementation Plan` heading. Stays here until the user explicitly requests a persona dispatch (Step 4) or signals readiness to file (Step 5).
4. **Step 4 — Persona Dispatch:** On explicit user request ("PM view?", "Tech Lead view?", "CTO view?"), summarizes the discussion as `CONVERSATION_SUMMARY` + `PROPOSED_PROBLEM_STATEMENT` and invokes the named sub-agent (`flow:pm`, `flow:tech-lead`, or `flow:cto`) via the Skill tool. Renders the agent's response verbatim. When the response is a `## SCOPE REFUSAL` block, the HARD-GATE prohibits auto-escalation, soft-re-prompting, and personal performance of the refused analysis.
5. **Step 5 — Wrap-up:** Captures the agreed problem statement (What/Why/Acceptance Criteria), runs the backwards-reasoning and include-bias scans, validates the body via `bin/flow validate-issue-body --mode vanilla`, and files the issue via `bin/flow issue` with `--label vanilla` (and never `--label decomposed`). The user's readiness signal is the single authorization to file; on validator failure, a bounded auto-fix loop (max 5 retries) corrects the body or halts with a structured `validator_max_retries` error. When the validator succeeds with a non-empty `warnings` array (code references — `src/` paths, `tests/` paths, `identifier::identifier` symbols — that slipped into the captured sections), a single-pass Code-Reference Warnings Handler renders the entries inline, translates them into user-visible prose, and re-validates before filing. The paired origin labels (`vanilla` here, `decomposed` for issues filed by `/flow:flow-plan #N`) make problem-statement vs. pre-planned provenance visible to `gh issue list`, the `/flow:flow-issues` dashboard, and any future label-based triage tooling.

---

## Personas

Persona dispatch routes to one of three planning sub-agents — each with its own scope authority and escalation target.

| Persona | Skill identifier | Scope authority | Escalates to |
|---------|------------------|-----------------|--------------|
| PM | `flow:pm` | Copy, content, small changes with no new functionality or complexity | Tech Lead |
| Tech Lead | `flow:tech-lead` | Extensions of existing modules, new code following established patterns, refactors within current architecture, test additions | CTO |
| CTO | `flow:cto` | Novel architectural decisions, around-the-corner problems, outside-the-box alternatives | Terminus — no further escalation |

---

## Gates

- **Step 1 Conversation Gate** — `/flow:flow-explore` invoked without a topic argument stops with usage guidance. No interactive prompt; the user re-runs the command with `<topic>`.
- **Step 3 Discussion Mode HARD-GATE** — forbids invocation of `decompose:decompose`, FLOW-PLAN sentinel markers, an `## Implementation Plan` heading, direct edits, commits, issue filing outside Step 5, inline draft issue body composition, `AskUserQuestion` self-prompts, and auto-dispatch to a planning sub-agent on inferred scope. Reading source files via the Read tool, grepping, and globbing is permitted; reading prior GitHub issues via `gh issue view` is permitted. The composition-discipline boundary (translate code-grounded findings into user-visible prose) is enforced as a soft scan at Step 5.
- **Step 4 Refusal Handling HARD-GATE** — when a sub-agent returns a `## SCOPE REFUSAL` block, the skill renders it verbatim and waits. Auto-escalation, soft-re-prompting, and personally performing the refused analysis are forbidden.
- **Step 5 Validator Gate** — the body must pass `bin/flow validate-issue-body --mode vanilla` before `bin/flow issue` runs. On validator failure, the skill applies a mechanical fix and re-runs the validator (max 5 attempts); after 5 failures the skill halts with a structured `validator_max_retries` error and the COMPLETE-FAILED banner without filing. On validator success with a non-empty `warnings` array, the single-pass Code-Reference Warnings Handler renders the flagged entries (pattern, line, snippet) inline, the model revises the body in working memory, re-validates, renders any residual warnings, and files.

---

## Output

A vanilla GitHub issue with three top-level sections: `## What`, `## Why`, `## Acceptance Criteria`. The issue carries the `vanilla` origin label (never `decomposed`) and no FLOW-PLAN sentinel markers. The user runs `/flow:flow-plan #N` next, which decomposes the problem statement into an implementation plan and re-plans the same issue in place (preserving the original problem statement above the FLOW-PLAN sentinel and attaching the `decomposed` label) ready for `/flow:flow-start #N`.
