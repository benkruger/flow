---
name: design
description: "Phase 3: Design — ask what we're building, propose 2-3 alternatives, get approval before any code. Can return to Research if gaps are found."
---

# FLOW Design — Phase 3: Design

<HARD-GATE>
Run this phase entry check as your very first action. If it exits
non-zero, stop immediately and show the error to the user.

```bash
python3 << 'PYCHECK'
import json, subprocess, sys
from pathlib import Path

def project_root():
    r = subprocess.run(['git', 'worktree', 'list', '--porcelain'],
                       capture_output=True, text=True)
    for line in r.stdout.split('\n'):
        if line.startswith('worktree '):
            return Path(line.split(' ', 1)[1].strip())
    return Path('.')

branch = subprocess.run(['git', 'branch', '--show-current'],
                        capture_output=True, text=True).stdout.strip()
state_file = project_root() / '.claude' / 'flow-states' / f'{branch}.json'

if not state_file.exists():
    print('BLOCKED: No FLOW feature in progress. Run /flow:start first.')
    sys.exit(1)

state = json.loads(state_file.read_text())
prev = state.get('phases', {}).get('2', {})
if prev.get('status') != 'complete':
    print('BLOCKED: Phase 2: Research must be complete before Design.')
    print('Run /flow:research first.')
    sys.exit(1)
PYCHECK
```
</HARD-GATE>

## Announce

Print:

```
============================================
  FLOW — Phase 3: Design — STARTING
============================================
```

## Update State

Read `.claude/flow-states/<branch>.json`. cd into the worktree.

Update Phase 3:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `3`

---

## Step 1 — What are we building?

Before proposing anything, ask the user to describe what they want.

Use AskUserQuestion with two questions:

**Question 1:** "What are we building?"
- New feature from scratch
- Enhancement to existing feature
- Changing existing behaviour
- Fixing a bug that needs design

**Question 2:** "Describe what you're building in detail. What should it do? What does success look like?"
- I'll describe it (type in Other)
- It's straightforward — same as the feature name
- It's complex — I'll explain the edge cases
- I have specific constraints to consider

Store the user's full description in `state["design"]["feature_description"]`.

---

## Step 2 — Read research findings

Read `state["research"]` from the state file:
- `affected_files` — what code will be touched
- `risks` — Rails-specific gotchas already discovered
- `clarifications` — decisions already made via Q&A
- `summary` — plain English of what exists

This is the foundation for your alternatives. Do not propose anything
that contradicts what Research found without flagging it explicitly.

---

## Step 3 — Propose 2-3 alternatives

Based on the feature description and research findings, propose 2-3
genuinely distinct approaches. Each alternative must address:

- **Approach summary** — 2-3 sentences describing the strategy
- **Schema changes** — new tables, columns, indexes for `data/release.sql`
- **Model changes** — Base/Create split, associations, callbacks
- **Controller / route changes** — subdomain, new routes, params pattern
- **Worker changes** — any async work, which queue
- **Key trade-offs** — what you gain and what you give up

Alternatives should be meaningfully different — not variations of the
same idea. If only one approach makes sense, explain why and present
it as the single recommendation.

---

## Step 4 — Validate alternatives via sub-agent

Launch a mandatory sub-agent to validate each alternative against the codebase.
Use the Task tool:

- `subagent_type`: `"Explore"`
- `description`: `"Design alternative validation"`

Provide these instructions to the sub-agent (fill in the details):

> You are validating design alternatives for the FLOW design phase.
> Feature: <feature name from state>
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
> - Feasibility: confirmed / blocked (with reason)
> - Conflicts found (if any)
> - Reuse opportunities (if any)
> - Files that would need modification (full paths)

Wait for the sub-agent to return. Incorporate its findings into the
alternatives before presenting them to the user in Step 5.

---

## Step 5 — Present alternatives

Include the sub-agent's validation findings in each alternative's markdown
preview — feasibility status, conflicts, and reuse opportunities.

Use AskUserQuestion with markdown previews — one option per alternative
plus a return-to-research option. Use the `markdown` field to show each
alternative's details in the preview panel.

```
Question: "Which approach should we take?"

Option A: [Short title]
  markdown preview:
    ## Approach
    [2-3 sentence summary]

    ## Schema
    [What changes in data/release.sql]

    ## Models
    [Base/Create decisions]

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

## Step 6 — Refine the chosen approach

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

## Step 7 — Present full design for approval

Show the complete design based on the chosen approach and refinements:

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

Then use AskUserQuestion:

> "Does this design look right?"
> - **Approve** — save and proceed to Plan
> - **Needs changes** — describe what to change, revise and re-present
> - **Go back to Research** — something fundamental is unclear

**If "Go back to Research"** — update state Phase 3 back to `pending`,
Phase 2 back to `in_progress`, then invoke `flow:research`.

---

## Step 8 — Save design to state

Write to `.claude/flow-states/<branch>.json` under `design`:

```json
"design": {
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

---

## Done — Update state and complete phase

Update Phase 3 in state:
1. `cumulative_seconds` += `current_time - session_started_at`
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `4`

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 3: Design is complete. Ready to begin Phase 4: Plan?"
> - **Yes, start Phase 4 now** — invoke `flow:plan`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 4 now" and "Not yet"

**If Yes**, print:

```
============================================
  FLOW — Phase 3: Design — COMPLETE
============================================
```

**If Not yet**, print:

```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```

---

## Hard Rules

- Never write or suggest any code during Design
- Always present at least 2 alternatives before allowing approval
- Always read Research findings before proposing anything
- If returning to Research, update state for both phases before invoking
