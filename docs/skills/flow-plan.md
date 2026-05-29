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
/flow:flow-plan <topic>
```

Produces a structured implementation plan and attaches it to a GitHub issue. The skill accepts two argument shapes: an issue reference (`#N`) re-plans the existing issue in place, and a bare prompt (`<topic>`) synthesizes a brief `## What` / `## Why` / `## Acceptance Criteria` from the planning conversation and files a new decomposed issue. In either mode the skill holds a Tech-Lead-default planning conversation, runs `decompose:decompose` against the agreed approach, transforms the synthesis into an Implementation Plan section wrapped in FLOW-PLAN sentinels, and finalizes via `gh issue edit ... --add-label decomposed` (issue-input mode) or `bin/flow issue --label decomposed --assignee @me` (bare-prompt mode).

**Multi-track filing (AC#4 of #1590).** When the skill runs in issue-input mode and the post-`decompose:decompose` DAG partitions into two or more disconnected components (zero cross-group dependency edges), Step 6 files one child decomposed issue per component instead of editing the source issue in place. Cross-component edges become `bin/flow link-blocked-by` links between children, and the source issue receives blocked-by links to each root child — it stays a plain problem statement (no Implementation Plan block, no `decomposed` label, not closed) and closes naturally via the AC#5 blocked-by cascade once every child PR merges. Bare-prompt mode is always single-track per AC#8: even when the DAG partitions cleanly, a bare-prompt invocation files exactly one issue.

The output is an issue ready for `/flow-start #N` (single-track re-planned issue), `/flow-start #M` (single-track new decomposed issue), or one `/flow-start` invocation per child (multi-track filing).

---

## What It Does

| Step | Name | Gate |
|------|------|------|
| 1 | Conversation Gate | HARD-GATE: argument required — accepts `#N` (regex `^#[1-9][0-9]*$`, issue-input mode) or a non-empty bare prompt (bare-prompt mode). No argument is rejected with usage guidance |
| 2 | Fetch Issue (issue-input mode only) | HARD-GATE: `gh issue view --json title,body,number,labels,state`; rejects closed issues with reopen-first guidance. The `decomposed` label is NOT a rejection criterion — re-planning in place is the correct path |
| 3 | Role Read | Reads `.flow.json` `role` field; Tech Lead is the default voice |
| 4 | Discussion Mode | HARD-GATE: codebase reads permitted; no inline draft Implementation Plan; no `AskUserQuestion` self-prompts; no auto-dispatch to a planning sub-agent |
| 5 | Persona Dispatch | HARD-GATE: render `## SCOPE REFUSAL` verbatim, no auto-escalation |
| 6 | Wrap-up | `decompose:decompose` invocation, Multi-Track Detection (issue-input mode only — branches to per-child filing when the DAG has ≥ 2 disconnected components per AC#4), transform to Implementation Plan wrapped in FLOW-PLAN sentinels, cognitively isolated Plan Review via `flow:plan-reviewer` with a capped (max 3 attempts) remediation loop routing per-finding to either re-decompose (task-DAG fixes) or revise-transform (in-place prose fixes), validate with `--mode decomposed`, then branch: issue-input single-track runs `gh issue edit <N> --body-file ... --add-label decomposed` (preserves content above the opening FLOW-PLAN sentinel and swaps the plan block); issue-input multi-track files one `bin/flow issue ... --label decomposed --assignee @me` per component and links them with `bin/flow link-blocked-by`; bare-prompt mode runs `bin/flow issue --title ... --label decomposed --assignee @me` (always single-track per AC#8). Clear the utility-in-progress marker |

1. **Step 1 — Conversation Gate:** Verifies that an argument was provided and routes to one of two modes. An `#N` argument routes to issue-input mode (Step 2 fetches the issue body); a bare non-empty prompt routes to bare-prompt mode (skip Step 2). No argument clears the utility-in-progress marker and stops with usage guidance.
2. **Step 2 — Fetch Issue (issue-input mode only):** Calls `gh issue view <N> --json title,body,number,labels,state`. Rejects closed issues with reopen-first guidance because `bin/flow plan-from-issue` refuses closed issues at flow-start and the resulting plan would be unusable. The `decomposed` label is not a rejection criterion — re-planning an already-decomposed issue is the in-place edit path. Bare-prompt mode skips this step.
3. **Step 3 — Role Read:** Reads `.flow.json` for the optional `role` field. Tech Lead is the default voice; the role only adjusts a one-line conversational note.
4. **Step 4 — Discussion Mode:** The default posture. Surfaces clarifying questions, reads source code via Read/Glob/Grep (unlike `/flow-explore` where source reads are forbidden), identifies risks and edge cases, iterates with the user. In bare-prompt mode the bare prompt seeds the conversation. Composing inline draft Implementation Plan sections is forbidden — the wrap-up step builds the plan from the decompose pass.
5. **Step 5 — Persona Dispatch:** On explicit user request ("PM view?", "Tech Lead view?", "CTO view?"), summarizes the discussion as `PARENT_ISSUE` + `CONVERSATION_SUMMARY` + `PROPOSED_APPROACH` and invokes the named sub-agent (`flow:pm`, `flow:tech-lead`, or `flow:cto`) via the Skill tool.
6. **Step 6 — Wrap-up:** Generates a session ID, invokes `decompose:decompose` against the agreed approach, transforms the synthesis into an Implementation Plan section wrapped in FLOW-PLAN sentinels, runs the backwards-reasoning and include-bias scans, runs a cognitively isolated **Plan Review** via `flow:plan-reviewer` (which audits the drafted plan against the `.claude/rules/` corpus with a remediation loop capped at 3 attempts, routing each finding to either re-decompose for task-DAG fixes or revise-transform for in-place prose fixes), validates the body via `bin/flow validate-issue-body --mode decomposed`, then branches on the Step 1 mode. **Issue-input mode** preserves every byte above the first opening FLOW-PLAN sentinel from the original issue body (or the whole body if no sentinel is present), strips any existing sentinel-delimited block, appends the fresh sentinel-wrapped `## Implementation Plan`, and edits the issue in place via `gh issue edit <N> --body-file ... --add-label decomposed`. **Bare-prompt mode** synthesizes a brief `## What` / `## Why` / `## Acceptance Criteria` from the conversation, appends the sentinel-wrapped plan, and files via `bin/flow issue --label decomposed --assignee @me`. On Plan Review cap exhaustion the skill files the issue with the last drafted plan and surfaces the final violations as a non-blocking advisory warning — the reviewer advises, it never blocks filing; on validator failure a bounded auto-fix loop (max 5 retries) corrects the body or halts with `validator_max_retries`.

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

- **Step 1 Conversation Gate** — rejects no-argument invocations with usage guidance naming both shapes (`#N` and bare prompt). The `#N` regex routes to issue-input mode; any non-empty argument that does not match the regex routes to bare-prompt mode.
- **Step 2 Fetch Gate (issue-input mode only)** — refuses to plan against closed issues with reopen-first guidance. The `decomposed` label is not a rejection criterion. Bare-prompt mode skips this step.
- **Step 4 Discussion Mode HARD-GATE** — forbids direct edits, commits, issue filing, inline draft Implementation Plan composition, `AskUserQuestion` self-prompts, and auto-dispatch to a planning sub-agent on inferred scope. Source-code reads are permitted (unlike `/flow-explore`).
- **Step 5 Refusal Handling HARD-GATE** — when a sub-agent returns a `## SCOPE REFUSAL` block, the skill renders it verbatim and waits. Auto-escalation, soft-re-prompting, and personally performing the refused analysis are forbidden.
- **Step 6 Plan Review Gate** — The drafted plan is audited by the cognitively isolated `flow:plan-reviewer` agent against the `.claude/rules/` corpus. The reviewer classifies each violation per-finding and returns one of three verdicts. A `re-decompose` verdict (any violation needs a task-DAG change) re-derives the plan through `decompose:decompose`; a `revise-transform` verdict (every violation is an orchestrator-authored prose artifact — table placement, a missing required table, doc-surface enumeration, wording) triggers an in-place Transform-step prose fix with no decompose re-run. Both remediation branches share a single cap of three attempts; if it is exhausted the issue is filed with the last drafted plan and the violations are surfaced to the user as a non-blocking advisory warning. The reviewer advises — it never blocks filing.
- **Step 6 Validator Gate** — the body must pass `bin/flow validate-issue-body --mode decomposed` before either `gh issue edit` (issue-input) or `bin/flow issue` (bare-prompt) runs. On validator failure, the skill applies a mechanical fix and re-runs the validator (max 5 attempts); after 5 failures the skill clears the utility marker, halts with a structured `validator_max_retries` error, and prints the COMPLETE-FAILED banner without filing or editing any issue.

---

## Output

An issue body with the same top-level sections in both modes (`## What`, `## Why`, `## Acceptance Criteria`, `## Implementation Plan` wrapped in FLOW-PLAN sentinels). The issue carries the `decomposed` label so `flow-issues` and `flow-orchestrate` recognize it as ready-for-flow-start work.

- **Issue-input single-track mode** edits the existing issue #N in place. The content above the opening FLOW-PLAN sentinel is preserved verbatim (the original problem statement); the plan block is swapped. The issue stays open and the assignee is not changed. The user runs `/flow-start #N` next.
- **Issue-input multi-track mode** files one new decomposed issue per disconnected DAG component (AC#4). Cross-component edges become `bin/flow link-blocked-by` links between children, and the source issue receives blocked-by links from each root child. The source issue stays a plain problem statement (no Implementation Plan block, no `decomposed` label, not closed); the AC#5 blocked-by cascade closes the source naturally after every child PR merges. The user runs `/flow-start #M_i` per child.
- **Bare-prompt mode** files a new decomposed issue assigned to the planner (`--assignee @me`). Always single-track per AC#8. The user runs `/flow-start #M` next.

In any case, `/flow-start` fetches the issue body, extracts the Implementation Plan section verbatim into `.flow-states/<branch>/plan.md`, opens the worktree and PR, and dispatches the Code phase against the plan tasks.
