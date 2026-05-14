---
title: /flow-plan
nav_order: 18
parent: Skills
---

# /flow-plan

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-plan #N
```

Decomposes a vanilla problem-statement issue (filed by `/flow-explore`) into a structured implementation plan and files it as a new decomposed GitHub issue. The skill reads the parent issue body, holds a Tech-Lead-default planning conversation, runs `decompose:decompose` against the agreed approach, transforms the synthesis into an Implementation Plan section wrapped in FLOW-PLAN sentinels, files the new issue with the `decomposed` label, and closes the parent vanilla issue with a comment naming the decomposed child.

The output is a decomposed issue ready for `/flow-start #M`. The vanilla issue stays as the durable problem statement; the decomposed issue carries the implementation plan that `bin/flow plan-from-issue` extracts at flow-start.

---

## What It Does

| Step | Name | Gate |
|------|------|------|
| 1 | Conversation Gate | HARD-GATE: `#N` argument required (regex `^#[1-9][0-9]*$`); bare-topic invocations rejected with migration message naming `/flow-explore` |
| 2 | Fetch Vanilla Issue | HARD-GATE: `gh issue view --json title,body,number,labels,state`; rejects issues already carrying `decomposed` label or in closed state |
| 3 | Role Read | Reads `.flow.json` `role` field; Tech Lead is the default voice |
| 4 | Discussion Mode | HARD-GATE: codebase reads permitted; no inline draft Implementation Plan; no `AskUserQuestion` self-prompts; no auto-dispatch to a planning sub-agent |
| 5 | Persona Dispatch | HARD-GATE: render `## SCOPE REFUSAL` verbatim, no auto-escalation |
| 6 | Wrap-up | `decompose:decompose` invocation, transform to Implementation Plan wrapped in FLOW-PLAN sentinels, validate with `--mode decomposed`, file with `--label decomposed` and `--assignee @me`, close parent via `bin/flow close-issue --comment`, clear marker |

1. **Step 1 — Conversation Gate:** Verifies the argument matches `#N`. Without an argument or with a bare-topic value, the gate clears the utility-in-progress marker and stops with migration guidance directing the user to `/flow-explore` for problem-statement filing first.
2. **Step 2 — Fetch Vanilla Issue:** Calls `gh issue view <N> --json title,body,number,labels,state`. Rejects already-`decomposed` issues (re-planning would file a sibling decomposed issue against an already-decomposed parent) and closed issues (require explicit reopen).
3. **Step 3 — Role Read:** Reads `.flow.json` for the optional `role` field. Tech Lead is the default voice; the role only adjusts a one-line conversational note.
4. **Step 4 — Discussion Mode:** The default posture. Surfaces clarifying questions, reads source code via Read/Glob/Grep (unlike `/flow-explore` where source reads are forbidden), identifies risks and edge cases, iterates with the user. Composing inline draft Implementation Plan sections is forbidden — the wrap-up step builds the plan from the decompose pass.
5. **Step 5 — Persona Dispatch:** On explicit user request ("PM view?", "Tech Lead view?", "CTO view?"), summarizes the discussion as `PARENT_ISSUE` + `CONVERSATION_SUMMARY` + `PROPOSED_APPROACH` and invokes the named sub-agent (`flow:pm`, `flow:tech-lead`, or `flow:cto`) via the Skill tool.
6. **Step 6 — Wrap-up:** Generates a session ID, invokes `decompose:decompose` against the agreed approach + parent body, transforms the synthesis into an Implementation Plan section wrapped in FLOW-PLAN sentinels, runs the backwards-reasoning and include-bias scans, validates the body via `bin/flow validate-issue-body --mode decomposed`, files the issue via `bin/flow issue --label decomposed --assignee @me` (assigning the decomposed issue to the planner who ran `flow-plan`), closes the parent vanilla issue with a comment naming the decomposed child via `bin/flow close-issue --comment`, and clears the marker. The user's readiness signal from Step 4 is the single authorization to file; on validator failure, a bounded auto-fix loop (max 5 retries) corrects the body or halts with `validator_max_retries`.

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

- **Step 1 Conversation Gate** — rejects no-argument and bare-topic invocations with a migration message directing the user to `/flow-explore`. No interactive prompt; the user re-runs the command with `#N`.
- **Step 2 Fetch Gate** — refuses to plan against issues that already carry the `decomposed` label or are closed. The user retargets to a vanilla problem-statement issue or reopens the closed issue first.
- **Step 4 Discussion Mode HARD-GATE** — forbids direct edits, commits, issue filing, inline draft Implementation Plan composition, `AskUserQuestion` self-prompts, and auto-dispatch to a planning sub-agent on inferred scope. Source-code reads are permitted (unlike `/flow-explore`).
- **Step 5 Refusal Handling HARD-GATE** — when a sub-agent returns a `## SCOPE REFUSAL` block, the skill renders it verbatim and waits. Auto-escalation, soft-re-prompting, and personally performing the refused analysis are forbidden.
- **Step 6 Validator Gate** — the body must pass `bin/flow validate-issue-body --mode decomposed` before `bin/flow issue` runs. On validator failure, the skill applies a mechanical fix and re-runs the validator (max 5 attempts); after 5 failures the skill clears the utility marker, halts with a structured `validator_max_retries` error, and prints the COMPLETE-FAILED banner without filing or closing the parent. The `bin/flow close-issue --comment` call fires after every successful filing — closing the parent at plan time is what makes the decomposed child the single open artifact for the problem.
- **Step 6 Close-Issue Failure Path** — when `bin/flow close-issue` returns `{"status":"error",...}` after the decomposed child has been filed, the skill halts without retrying the closure or clearing the utility-in-progress marker silently. The decomposed child #M is already on the issue tracker; the parent #N remains open. The skill reports both numbers and the underlying gh error to the user with a concrete `gh issue close` recovery command, prints the COMPLETE-FAILED banner, and stops. The user reconciles the GitHub state by closing the parent manually before running `/flow-start #M`.

---

## Output

A decomposed GitHub issue with five top-level sections (`## What`, `## Why`, `## Acceptance Criteria`, `## Implementation Plan` wrapped in FLOW-PLAN sentinels, `## Parent Issue`) labeled `decomposed` and assigned to the planner who ran `flow-plan` (via `--assignee @me`). The parent vanilla issue is closed at filing time with a comment naming the new decomposed child, so the decomposed work is the single open artifact for the problem. The user runs `/flow-start #M` next, which fetches the issue body, extracts the Implementation Plan section verbatim into `.flow-states/<branch>/plan.md`, opens the worktree and PR, and dispatches the Code phase against the plan tasks.
