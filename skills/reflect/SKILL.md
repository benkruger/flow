---
name: reflect
description: "Phase 7: Reflect — review what went wrong, capture learnings in CLAUDE.md, note plugin improvements. Runs before the PR is merged. The only commits are CLAUDE.md and .claude/ changes."
model: sonnet
---

# FLOW Reflect — Phase 7: Reflect

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.6.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 6: Review must be
     complete. Run /flow:review first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.5 — Phase 7: Reflect — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 7:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `7`

## Logging

No logging for this phase. Reflect runs no Bash commands beyond the entry
gate — there is nothing to log.

---

## Step 1 — Gather all sources

Read and synthesise from three sources before asking the user anything:

### Source A — State file data

For each phase, note:
- `visit_count` > 1 → this phase had friction, was revisited
- `cumulative_seconds` — note the time each phase took for context
- `state["notes"]` → explicit corrections captured during the session
- `state["research"]["risks"]` → risks found, check if any caused problems
- `state["research"]["open_questions"]` → anything that was unresolved
- `state["design"]["rationale"]` → why this approach was chosen, did it hold up?
- Plan sections that needed multiple revisions
- Review findings that were caught late

### Source B — Captured notes

Read `state["notes"]` in full. These are corrections and learnings
captured during the session via `/flow:note`. They are the most direct
signal of what went wrong.

### Source C — Conversation context

Review the current conversation for:
- Moments where the user corrected Claude
- Responses where Claude was overruled or pushed back
- Misunderstandings that required clarification
- Suggestions Claude made that were rejected

Note: context may have been compacted. Use what is available.
Sources A and B are the guaranteed record.

---

## Step 2 — Follow the reflection process

With the evidence gathered in Step 1, follow the reflection process below.

When Step E says to commit, use `/flow:commit`. The commit goes onto the
feature branch so CLAUDE.md improvements merge to main with the feature.

### Step A — Synthesize findings

Before asking the user anything, organize all gathered evidence into four
categories:

**Process violations** — existing rules in CLAUDE.md that were broken or
nearly broken during the session. Quote the specific rule.

**Claude mistakes** — things Claude got wrong that the user had to correct.
Be specific and honest. Name the mistake clearly — do not soften or hedge.

For each mistake, state:
1. What Claude did wrong (the actual behavior, not a euphemism)
2. What the user said or did to correct it (quote or paraphrase)
3. How many rounds of correction it took before Claude got it right

If you cannot answer all three, you are probably softening the mistake.

**Missing rules** — situations where Claude did the wrong thing but no
existing rule covered it. These are gaps in CLAUDE.md.

**Process gaps** — places where the development process itself (tools,
skills, workflows) should be improved. These are not CLAUDE.md rules —
they are process changes.

### Step B — Present findings

Present the synthesis to the user in a banner:

````markdown
```text
============================================
  Reflect — Findings
============================================

  Process violations
  ------------------
  - CLAUDE.md says "never use guard clauses" but Claude
    added an early return in the worker
  - ...

  Claude mistakes
  ---------------
  - Suggested git rebase (forbidden — corrected immediately)
  - ...

  Missing rules
  -------------
  - No rule about checking eager-loaded associations
    before using pluck
  - ...

  Process gaps
  ------------
  - /flow:commit should warn when branch is behind
  - ...

============================================
```
````

Then use AskUserQuestion:

> "Does this capture what went wrong? Anything I missed or got wrong?"
>
> - **Yes, this is accurate** — proceed to proposals
> - **Needs corrections** — describe what to change

If "Needs corrections", revise and re-present until accurate.

### Step C — Propose CLAUDE.md additions

For each item in "Missing rules", propose a specific addition to CLAUDE.md.

**Writing rules for CLAUDE.md:**
- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session
- Placed in the correct section of the target CLAUDE.md

Present each proposal individually using AskUserQuestion:

> "Proposed CLAUDE.md addition:
> '[proposed text]'
> Section: [target section]"
>
> - **Yes, add it**
> - **Yes, but rephrase** — describe how
> - **No, skip this one**

For "Yes, but rephrase" — revise and confirm before collecting.

Collect all approved additions. Do not apply yet.

### Step D — Strengthen violated rules

For each item in "Process violations", evaluate whether the existing rule's
language was clear enough. If the violation happened because the rule was
ambiguous or easy to overlook, propose a rewording.

Present each rewording proposal individually using AskUserQuestion (same
three options as Step C).

Collect all approved rewordings. Do not apply yet.

### Step E — Apply approved changes

Read the target CLAUDE.md. Apply all approved additions and rewordings.
Do not duplicate existing content.

Then commit via `/flow:commit`.

Only CLAUDE.md and `.claude/` files are committed — never application code.

---

## Step 3 — Plugin improvement notes

Present the plugin gaps inside a fenced code block:

````markdown
```text
============================================
  FLOW — Plugin Improvements to Consider
============================================

  These are improvements to the FLOW process itself.
  They are not committed — review and open issues on
  the plugin repo if you want to address them.

  - Research skill: explicitly ask about Sidekiq queue names
  - Plan skill: prompt for multi-session git workflow awareness
  - flow:commit: add note about branch-behind being common

============================================
```
````

Use AskUserQuestion:

> "Would you like to add anything to the plugin improvement notes
> before we close out Reflect?"
>
> - **No, that's everything**
> - **Yes, add this** — describe in Other

---

## Done — Update state and complete phase

Update Phase 7 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `8`

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.8.5 — Phase 7: Reflect — COMPLETE (<formatted_time>)
  Merge the PR, then run /flow:cleanup.
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 7: Reflect is complete. The PR now includes CLAUDE.md improvements. Ready to begin Phase 8: Cleanup?"
>
> - **Yes, start Phase 8 now** — invoke `flow:cleanup`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 8 now" and "Not yet"

**If Yes** — invoke `flow:cleanup` using the Skill tool.

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

- Never commit application code in Reflect — only CLAUDE.md and .claude/
- Always read all three sources before presenting findings
- Follow the reflection process (Steps A through E) exactly — do not skip or reorder steps
- Plugin improvement notes are presented only — never committed
