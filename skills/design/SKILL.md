---
name: design
description: "Phase 3: Design — ask what we're building, propose 2-3 alternatives, get approval before any code. Can return to Research if gaps are found."
model: opus
---

# FLOW Design — Phase 3: Design

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.2.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 2: Research must be
     complete. Run /flow:research first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 3: Design — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 3:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `3`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

## Logging

No logging for this phase. Design runs no Bash commands beyond the entry
gate — the work is AskUserQuestion calls, a sub-agent exploration, and
state file writes.

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for alternatives structure, validation sub-agent prompt,
design presentation format, and design object schema.

### If Rails

#### Alternatives Structure

Each alternative must address:

- **Approach summary** — 2-3 sentences describing the strategy
- **Schema changes** — new tables, columns, indexes for `data/release.sql`
- **Model changes** — Base/Create split, associations, callbacks
- **Controller / route changes** — subdomain, new routes, params pattern
- **Worker changes** — any async work, which queue
- **Key trade-offs** — what you gain and what you give up

#### Validation Sub-Agent Prompt

Provide these instructions to the Step 3 sub-agent (fill in the details):

> You are validating design alternatives for the FLOW design phase.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> Research findings: <paste state["research"] summary, affected_files, risks>
>
> Alternatives to validate:
> <paste the 2-3 alternatives drafted in Step 3>
>
> For each alternative, check the codebase:
>
> 1. **Feasibility** — Do the files it would touch exist? Does the route
>    structure support it? Does the schema allow it?
> 2. **Conflicts** — Any naming collisions with existing code? Callback
>    chains that would interfere? Existing logic that contradicts the approach?
> 3. **Reuse opportunities** — Existing helpers, shared modules, or patterns
>    that this alternative could leverage instead of building from scratch?
> 4. **Files to modify** — Exact list of files each alternative would need
>    to create or modify.
>
> Return per-alternative:
>
> - Feasibility: confirmed / blocked (with reason)
> - Conflicts found (if any)
> - Reuse opportunities (if any)
> - Files that would need modification (full paths)

#### Design Presentation Format

Show the complete design inside a fenced code block:

````text
```
============================================
  FLOW — Phase 3: Design — PROPOSAL
============================================

  Feature     : <feature description>
  Approach    : <chosen approach title>

  Schema Changes
  --------------
  <list of tables/columns/indexes — or "None">

  Model Changes
  -------------
  <Base/Create decisions, associations, callbacks>

  Controller / Route Changes
  --------------------------
  <subdomain, route, params pattern>

  Worker Changes
  --------------
  <queue, structure — or "None">

  Risks
  -----
  <risks from research that are relevant to this approach>

============================================
```
````

#### Design Object Schema

Write to `.flow-states/<branch>.json` under `design`:

```json
{
  "feature_description": "<user's own words from Step 1>",
  "chosen_approach": "<approach title>",
  "rationale": "<why this approach was chosen>",
  "schema_changes": [],
  "model_changes": [],
  "controller_changes": [],
  "worker_changes": [],
  "route_changes": [],
  "risks": [],
  "approved_at": "<current UTC timestamp>"
}
```

### If Python

#### Alternatives Structure

Each alternative must address:

- **Approach summary** — 2-3 sentences describing the strategy
- **Module changes** — new modules, modified modules, imports
- **Test changes** — new test files, fixtures needed
- **Script changes** — CLI scripts, entry points, argument parsing
- **Key trade-offs** — what you gain and what you give up

#### Validation Sub-Agent Prompt

Provide these instructions to the Step 3 sub-agent (fill in the details):

> You are validating design alternatives for the FLOW design phase.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> Research findings: <paste state["research"] summary, affected_files, risks>
>
> Alternatives to validate:
> <paste the 2-3 alternatives drafted in Step 3>
>
> For each alternative, check the codebase:
>
> 1. **Feasibility** — Do the files it would touch exist? Does the module
>    structure support it? Are dependencies available?
> 2. **Conflicts** — Any naming collisions with existing code? Circular
>    import risks? Existing logic that contradicts the approach?
> 3. **Reuse opportunities** — Existing utilities, shared modules, or patterns
>    that this alternative could leverage instead of building from scratch?
> 4. **Files to modify** — Exact list of files each alternative would need
>    to create or modify.
>
> Return per-alternative:
>
> - Feasibility: confirmed / blocked (with reason)
> - Conflicts found (if any)
> - Reuse opportunities (if any)
> - Files that would need modification (full paths)

#### Design Presentation Format

Show the complete design inside a fenced code block:

````text
```
============================================
  FLOW — Phase 3: Design — PROPOSAL
============================================

  Feature     : <feature description>
  Approach    : <chosen approach title>

  Module Changes
  --------------
  <new/modified modules, imports>

  Test Changes
  ------------
  <new test files, fixtures — or "None">

  Script Changes
  --------------
  <CLI scripts, entry points — or "None">

  Risks
  -----
  <risks from research that are relevant to this approach>

============================================
```
````

#### Design Object Schema

Write to `.flow-states/<branch>.json` under `design`:

```json
{
  "feature_description": "<user's own words from Step 1>",
  "chosen_approach": "<approach title>",
  "rationale": "<why this approach was chosen>",
  "module_changes": [],
  "test_changes": [],
  "script_changes": [],
  "risks": [],
  "approved_at": "<current UTC timestamp>"
}
```

---

## Step 1 — Review research and scope the design

Before proposing anything, review the research findings from state:
- `summary` — what exists
- `affected_files` — what code will be touched
- `risks` — gotchas discovered
- `clarifications` — decisions already made

Present a brief summary to the user:

> "Research found: [summary]. [N] files affected. [N] risks identified."

Then ask targeted questions that build on the research:

Use AskUserQuestion with two questions:

**Question 1:** "What type of change is this?"
- New feature from scratch
- Enhancement to existing feature
- Changing existing behaviour
- Fixing a bug that needs design

**Question 2:** "Based on the research findings above, describe what success looks like. What should change from the current behavior?"
- I'll describe it (type in Other)
- It's straightforward — same as the feature name
- It's complex — I'll explain the edge cases
- I have specific constraints to consider

Store the user's description in `state["design"]["feature_description"]`.

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

Do not propose anything that contradicts what Research found without
flagging it explicitly.

---

## Step 2 — Propose 2-3 alternatives

Based on the feature description and research findings, propose 2-3
genuinely distinct approaches. Structure each alternative using the
**Alternatives Structure** from the framework section above.

Alternatives should be meaningfully different — not variations of the
same idea. If only one approach makes sense, explain why and present
it as the single recommendation.

---

## Step 3 — Validate alternatives via sub-agent

Launch a mandatory sub-agent to validate each alternative against the codebase.
Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Design alternative validation"`

Provide the sub-agent with the **Validation Sub-Agent Prompt** from the
framework section above (fill in the feature name, research findings, and
alternatives).

Wait for the sub-agent to return. Incorporate its findings into the
alternatives before presenting them to the user in Step 5.

---

## Step 4 — Present alternatives

Include the sub-agent's validation findings in each alternative's markdown
preview — feasibility status, conflicts, and reuse opportunities.

Use AskUserQuestion with markdown previews — one option per alternative
plus a return-to-research option. Use the `markdown` field to show each
alternative's details in the preview panel.

```text
Question: "Which approach should we take?"

Option A: [Short title]
  markdown preview:
    ## Approach
    [2-3 sentence summary]

    ## Changes
    [Key changes by category — use framework section categories]

    ## Trade-offs
    + [Pro]
    - [Con]

Option B: [Short title]
  [same structure]

Option C: [Short title]
  [same structure]

Option D: Need more research first
```

**If "Need more research first"** — update state Phase 3 back to
`pending`, Phase 2 back to `in_progress`, then invoke `flow:research`.

---

## Step 5 — Refine the chosen approach

Based on the selection, ask targeted follow-up questions about the
chosen approach only. Use AskUserQuestion in batches of up to 4.

Good follow-up questions:
- Specific schema decisions ("Should X have a unique index?")
- Edge case handling ("What happens if Y is nil?")
- Naming decisions if non-obvious
- Priority of trade-offs ("Is performance or simplicity more important here?")

Only ask what is genuinely unclear. Do not pad with questions that
have obvious answers from the research findings.

---

## Step 6 — Present full design for approval

Show the complete design based on the chosen approach and refinements using
the **Design Presentation Format** from the framework section above.

Then use AskUserQuestion:

> "Does this design look right?"
>
> - **Approve** — save and proceed to Plan
> - **Needs changes** — describe what to change, revise and re-present
> - **Go back to Research** — something fundamental is unclear

**If "Go back to Research"** — update state Phase 3 back to `pending`,
Phase 2 back to `in_progress`, then invoke `flow:research`.

---

## Step 7 — Save design to state

Write the design to `.flow-states/<branch>.json` under `design` using the
**Design Object Schema** from the framework section above.

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

---

## Done — Update state and complete phase

Update Phase 3 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `4`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

For the banner below, compute `<formatted_time>` from the integer `cumulative_seconds` stored above: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60. Do not write the formatted string back to the state file.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 3: Design — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 3: Design is complete. Ready to begin Phase 4: Plan?"
>
> - **Yes, start Phase 4 now** — invoke `flow:plan`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 4 now" and "Not yet"

**If Yes** — invoke `flow:plan` using the Skill tool.

**If Not yet**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Never write or suggest any code during Design
- Always present at least 2 alternatives before allowing approval
- Always read Research findings before proposing anything
- If returning to Research, update state for both phases before invoking
