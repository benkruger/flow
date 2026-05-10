---
name: flow-create-issue
description: "Capture a brainstormed solution as a pre-planned issue with an Implementation Plan section for fast-tracking through the Plan phase."
---

# Flow Create Issue

Capture a brainstormed solution from the current conversation and file it as a pre-planned GitHub issue. The issue includes an Implementation Plan section (Context, Exploration, Risks, Approach, Dependency Graph, Tasks) that the Plan phase extracts directly — no re-derivation needed.

This skill requires prior brainstorming context in the conversation. The user must have already explored the problem (typically via `/decompose:decompose`) and iterated on a solution before invoking this skill.

## Usage

```text
/flow:flow-create-issue
/flow:flow-create-issue --force-decompose
```

- `/flow:flow-create-issue` — start from the Conversation Gate
- `/flow:flow-create-issue --force-decompose` — force a fresh decompose even when prior implementation-focused output exists in the conversation

## Concurrency

This skill creates shared GitHub state (issues). Issue creation is
idempotent by title — if an issue with the same title already exists,
the user should be warned before filing a duplicate.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-create-issue — STARTING
──────────────────────────────────────────────────
```
````

Immediately after the banner, capture the active Claude Code
session_id and write the per-session "utility skill in progress"
marker so the Stop hook refuses turn-end while this skill is running.
Without the marker the model returns control to the user when the
decompose:decompose Skill tool returns mid-pipeline, breaking the
unattended-flow contract this skill promises.

Capture the session_id ONCE here. Reading the SessionStart capture
file on every set/clear call is a race surface: a concurrent Claude
Code session's SessionStart overwrites the capture file mid-skill,
so set-time and clear-time would resolve to different session_ids
and the marker would orphan. Pass the captured value explicitly to
every set/clear invocation below — including the error-exit paths
in the Conversation Gate and the File Cancel branch.

Run `bin/flow current-session-id` and capture its stdout as the
literal `<session_id>` to substitute into every subsequent
set-utility-in-progress and clear-utility-in-progress invocation:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow current-session-id
```

If the captured value is empty (no SessionStart capture file
present), skip the set call entirely — proceed without the marker.
The Stop hook treats a missing marker as a non-block, so the skill
runs without protection but does not break.

When the captured value is non-empty, write the marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-create-issue --session-id <session_id>
```

---

## Conversation Gate

Before entering the pipeline, verify that the current conversation contains
brainstorming context — a problem that was explored, a solution that was
discussed and agreed upon. This skill captures solutions, it does not
discover them.

**Signals that context exists** — proceed to Capture:

- Prior `/decompose:decompose` output in the conversation
- Extended back-and-forth about a problem and its solution
- An agreed approach, design, or set of changes discussed
- The user explicitly says "file it", "create an issue", or similar

**Signals that context is missing** — reject:

- The skill was invoked with a bare problem description and no prior discussion
- No decompose output or design iteration is visible in the conversation
- The conversation just started with this invocation

<HARD-GATE>

If no brainstorming context exists, clear the utility-in-progress
marker so the Stop hook does not refuse turn-end after the rejection,
then output this guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue --session-id <session_id>
```

> "This skill captures a brainstormed solution as a pre-planned issue.
> Start by running `/decompose:decompose` to research the problem,
> iterate on a solution, then invoke `/flow:flow-create-issue` when
> you have an agreed approach."

Do not proceed to Capture, propose direct edits, commit changes, or take
any action outside this skill without brainstorming context in the
conversation.

</HARD-GATE>

---

## Capture

Generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool. This ID
scopes the body file path (`.flow-issue-body-<id>`) so concurrent
`flow-create-issue` invocations cannot collide on the same temp file.

**Capture the problem sections** from the conversation context. Synthesize
the discussion into these structured sections in working memory — do not
re-analyze or re-explore, just distill what was already discussed:

- **Problem** — What is broken, missing, or inadequate. Include observable
  behavior, evidence from the codebase (file paths, line numbers), and user
  impact. Grounded in the exploration already done in the conversation.
- **Acceptance Criteria** — Binary, testable conditions. Pass/fail with no
  subjective judgment.
- **Files to Investigate** — Real file paths verified during the conversation's
  codebase exploration. Include a brief note on why each is relevant.
- **Out of Scope** — Explicit boundaries to prevent scope creep.
- **Context** — Business reason, architectural constraints, or design decisions.

---

## Title Authoring

The issue title flows downstream into the branch name (via
`branch_name`), the PR title (via `derive_feature`), the commit
subject, and the TUI feature line — every user-visible surface
inherits whatever you write here. Titles must read as plain English
to a stakeholder who is not a contributor; titles that smuggle in
code symbols, internal acronyms, or one-letter shorthand corrupt
every downstream surface they reach.

### Required

Titles must describe the user-visible problem or outcome in plain
English. Subject + verb + object as a reader would say it out loud.
A non-contributor reading the title in a release-notes feed should
understand what the change is for without consulting the codebase.

### Forbidden

The following must not appear in the title — they belong in the
issue body, the plan, or the code, never in the headline string:

- **Code symbols** — function names, type names, identifiers like
  `code_tasks_total`, command names like `bin/flow`.
- **Field names and file paths** — `state["foo"]`, `src/utils.rs`,
  any `module::function` reference.
- **Line numbers** — `:42`, `lines 100-120`.
- **Internal acronyms without expansion** — TUI, DAG, RAII,
  sentinel, hash, gate, agent shorthand. Expand on first use, or
  paraphrase entirely.
- **One-letter shorthand** — `X-of-Y`, `M of N`, single-letter
  variable names.
- **Abbreviations a non-contributor would not recognize** — repo-
  specific jargon, internal product code-names, in-flight
  refactor labels.

### Bad → Good Examples

| Bad (what flow-create-issue produces today) | Good (what the rule requires) |
|---|---|
| Wire code_tasks_total writer and put X-of-Y first in Code-phase TUI annotation | Show task progress as "step 3 of 7" in the Code phase status display |
| Fix three-hook deadlock on shared-config edits in autonomous flows | Stop the abort skill from deadlocking when a flow edits shared config |
| Add structural code_read field to pre-mortem agent finding schema | Have the pre-mortem agent record which files it read for each finding |

The title is the seed for every downstream identifier the user
will see. A title that fails this rule produces an unreadable
branch, an unreadable PR title, an unreadable commit subject, and
an unreadable TUI line — fixing the title at the source is much
cheaper than patching every downstream surface.

---

## Decompose

Check the conversation for **substantive exploration** of the problem
and solution. Substantive exploration contains all three signals:

- **Named files** — the conversation references specific file paths
  in the codebase that the change will touch
- **Identified root cause** — specific code references, line numbers,
  or a concrete bug mechanism (not just symptoms or speculation)
- **Agreed approach** — the user has confirmed direction on how to
  proceed (a chosen design, a concrete plan of attack)

A prior `/decompose:decompose` invocation in the conversation is a
strong signal of substantive exploration but is not required —
extended back-and-forth that produces the three signals above
qualifies on its own.

**If the conversation contains substantive exploration AND
`--force-decompose` was NOT passed:** the existing context is
sufficient. Skip the decompose invocation below and proceed
directly to Transform + Draft.

**If the conversation lacks one or more of the substantive-exploration
signals, or `--force-decompose` was passed:** invoke
`decompose:decompose` via the Skill tool with an implementation-focused
prompt. The prompt must make clear that the problem and solution are
already agreed — decompose should structure the implementation into
tasks, not re-analyze the problem.

Example prompt structure:

> "Given the following agreed solution, decompose the implementation into
> ordered tasks with dependencies, approach, and file targets. The problem
> is already understood — focus on structuring the work.
>
> [Summary of the agreed solution from the conversation]
>
> [Key files and patterns identified during brainstorming]"

The decompose output produces a structured DAG with nodes, dependencies,
and a synthesis — this becomes the foundation for the Implementation Plan.

<HARD-GATE>

When the Skill tool returns from the decompose:decompose invocation,
you are still inside flow-create-issue. The Skill tool's return is
NOT a stopping point — it is a mid-skill handoff. Do not stop, do
not summarize, do not ask the user "want me to continue?", do not
return control to the user. Proceed immediately to Transform + Draft
below using the decompose output you just received.

If you stop here, the user must prompt you again to continue, which
breaks the unattended flow that flow-create-issue promises to its
consumers. The whole point of the skill is that one invocation
produces a filed issue without further user input.

This gate fires whether the Decompose step invoked decompose:decompose
or skipped it. Either path lands at Transform + Draft as the next
action — no pause, no acknowledgement, no summary.

</HARD-GATE>

---

## Transform + Draft

Take the decompose synthesis from the conversation — either from a
prior `/decompose:decompose` invocation (when the Decompose step
skipped a fresh invocation) or from the invocation you just ran — and
transform it into an Implementation Plan section that matches the plan
file format used by `flow-plan`. The Implementation Plan must contain
these subsections:

- **Context** — What the user wants to build and why
- **Exploration** — What exists in the codebase, affected files, patterns discovered
- **Risks** — What could go wrong, edge cases, constraints
- **Approach** — The chosen approach and rationale
- **Dependency Graph** — Table of tasks with types and dependencies:

```markdown
| Task | Type | Depends On |
|------|------|------------|
| 1. Write tests | test | — |
| 2. Implement feature | implement | 1 |
```

- **Tasks** — Ordered implementation tasks, each with:
  - Description of what to build
  - Files to create or modify
  - TDD notes (what the test should verify)

Tasks must use `#### Task N:` heading format (these become `### Task N:`
headings in the plan file after heading promotion by `flow-plan`).

### Combine into Issue Body

Combine the captured problem sections with the Implementation Plan
into a single issue body in working memory. The section order must be:

**Problem** (from capture) → **Acceptance Criteria** (from capture) →
**Implementation Plan** (from transform, wrapped between sentinels —
containing Context, Exploration, Risks, Approach, Dependency Graph,
Tasks subsections) → **Files to Investigate** (from capture) →
**Out of Scope** (from capture) → **Context** (from capture —
business reason).

Each top-level section uses `##` headings. The Implementation Plan's
subsections use `###` headings. Task entries within the Tasks subsection
use `####` headings.

**Wrap the Implementation Plan in FLOW-PLAN sentinels.** Place the
literal HTML comment `<!-- FLOW-PLAN-BEGIN -->` on its own line
immediately before the `## Implementation Plan` heading, and the
literal HTML comment `<!-- FLOW-PLAN-END -->` on its own line
immediately after the last Task entry (before the next `## ` heading).
The sentinels delimit the bytes that `bin/flow plan-from-issue` will
extract verbatim and write to `.flow-states/<branch>/plan.md` when the
issue is later picked up via `/flow:flow-start #N`. Without the
sentinel pair, plan-from-issue rejects the issue with
`plan_markers_missing` and the flow halts.

The wrapped block looks like this in the issue body:

```markdown
<!-- FLOW-PLAN-BEGIN -->
## Implementation Plan

### Context
...

### Exploration
...

### Tasks

#### Task 1: ...
...
<!-- FLOW-PLAN-END -->
```

### Draft Presentation

Present the full draft inline in the response — both title and body. Do
not tell the user to look at a file. Render it as a formatted markdown
block so the user can review every detail.

---

## File

<HARD-GATE>

After presenting the draft, ask the user to confirm via AskUserQuestion
with structured parameters:

- **question**: "Review the draft above. Ready to file?"
- **header**: "File Issue"
- **options**:
  - label: "File issue", description: "File against the current repo with the decomposed label"
  - label: "Revise draft", description: "Edit the draft based on your feedback"
  - label: "Cancel", description: "Stop without filing an issue"

Do not file the issue, propose direct edits, commit changes, or take
any action outside this skill without explicit user approval via
AskUserQuestion — even if the answer appears obvious from context.

**If "File issue"** → proceed to Filing below.

**If "Revise draft"** → revise based on the user's feedback and
re-present the draft. If the feedback is substantial (changes the
problem understanding or approach), re-run `decompose:decompose` with
the updated understanding and re-transform. If the feedback is
editorial (wording, scope adjustments), edit the draft directly.
**When in doubt, treat the feedback as substantial and re-run
`decompose:decompose`** — the safe default is the conservative action
(per `.claude/rules/skill-authoring.md` "Safe Defaults for Subjective
Classification"); editing a draft built on a misaligned decompose ships
an incorrect Implementation Plan. After revising, re-present the draft
and ask the same AskUserQuestion. Iterate as many times as needed.

**If "Cancel"** → clear the utility-in-progress marker so the Stop
hook does not refuse turn-end after cancellation, then stop without
filing. Do not write the body file. Do not output the COMPLETE
banner.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue --session-id <session_id>
```

</HARD-GATE>

---

## Filing

Write the issue body to `.flow-issue-body-<id>` in the project root
using the Write tool. Then file it against the current repo (no
`--repo` flag — `flow-create-issue` always files where the user is):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id> --label decomposed
```

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

Clear the utility-in-progress marker so the Stop hook stops refusing
turn-end now that the skill has completed its work:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue --session-id <session_id>
```

Display the issue URL to the user, then output the COMPLETE banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-create-issue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never file an issue without explicit user approval — the AskUserQuestion before filing is the mandatory gate
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- The issue body must be self-contained — a fresh session with no memory of this conversation must be able to execute it
- Always use the Write tool to create body files (`.flow-issue-body-<id>`) — never pass body text as a CLI argument
- Never delete the body file — the `bin/flow issue` script handles cleanup
- The Implementation Plan section must use heading levels that match the plan file format after promotion by `flow-plan` (### in the issue becomes ## in the plan file)
