---
name: reflect
description: "Phase 8: Reflect — review what went wrong, capture learnings, route each to its correct permanent home. Runs before the PR is merged. The only commits are CLAUDE.md and .claude/ changes."
model: sonnet
---

# FLOW Reflect — Phase 8: Reflect

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.7.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 7: Security must be
     complete. Run /flow:security first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.11.0 — Phase 8: Reflect — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 8:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `8`

## Logging

No logging for this phase. Reflect runs no Bash commands beyond the entry
gate — there is nothing to log.

---

## Step 1 — Gather all sources

Read and synthesise from four sources before asking the user anything:

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

### Source D — Worktree auto-memory

Claude writes auto-memory during feature work to a path scoped to the
worktree. This memory will be lost when Cleanup removes the worktree.

1. Read `state["worktree"]` to get the worktree absolute path
2. Escape the path: replace `/` with `-`, drop the leading `-`
   (e.g. `/Users/ben/code/hh/.worktrees/my-feature` becomes
   `Users-ben-code-hh-.worktrees-my-feature`)
3. Read `~/.claude/projects/<escaped-path>/memory/MEMORY.md`
4. If it exists, include its contents as evidence alongside Sources A-C
5. If it does not exist (no auto-memory was written), skip silently

---

## Step 2 — Synthesize findings

Before asking the user anything, organize all gathered evidence into five
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

**Worth preserving** — items from the worktree auto-memory (Source D) that
contain useful patterns, observations, or context that future sessions
should know. Filter for durable value — not everything in auto-memory is
worth keeping. Skip this category if Source D was empty or did not exist.

## Step 3 — Present findings

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

  Worth preserving (from worktree memory)
  ----------------------------------------
  - Tests with Time.zone.now fail near midnight
  - ...

============================================
```
````

Omit the "Worth preserving" section from the banner if Source D was empty
or had nothing worth keeping.

Then use AskUserQuestion:

> "Does this capture what went wrong? Anything I missed or got wrong?"
>
> - **Yes, this is accurate** — proceed to proposals
> - **Needs corrections** — describe what to change

If "Needs corrections", revise and re-present until accurate.

## Step 4 — Propose additions with destination routing

For each item in "Missing rules" and "Worth preserving", propose a specific
addition and recommend a destination.

### The 5 destinations

| # | Name | Path | Write method |
|---|------|------|-------------|
| 1 | Global CLAUDE.md | `~/.claude/CLAUDE.md` | Edit directly |
| 2 | Project CLAUDE.md | `CLAUDE.md` in worktree | Edit, commit via `/flow:commit` |
| 3 | Global rules | `~/.claude/rules/<topic>.md` | Edit directly |
| 4 | Project rules | `.claude/rules/<topic>.md` in worktree | Edit, commit via `/flow:commit` |
| 5 | Project memory | `~/.claude/projects/<repo-root>/memory/MEMORY.md` | Edit directly |

Destinations 1, 3, 5 are user-private (outside the repo, not committed).
Destinations 2, 4 are committed to the feature branch via PR.

### Routing heuristics

| Learning type | Recommended destination |
|---|---|
| Process/behavior rule ("always X before Y") | 1 — Global CLAUDE.md |
| Project architecture discovery | 2 — Project CLAUDE.md |
| Universal coding style or anti-pattern | 3 — Global rules |
| Project-specific coding gotcha | 4 — Project rules |
| Informal pattern, observation, ephemeral note | 5 — Project memory |

### Per-proposal workflow

**Writing rules for CLAUDE.md and rules files:**
- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session
- Placed in the correct section of the target file

Present each proposal individually using AskUserQuestion:

> "Proposed addition:
> '[proposed text]'
> Section: [target section]
> Recommended destination: [name] ([path])"
>
> - **Yes, add to [recommended destination]**
> - **Yes, but different destination** — will ask which
> - **Yes, but rephrase** — describe how
> - **No, skip this one**

For "Yes, but different destination" — present the 5-destination list and
ask which one. For "Yes, but rephrase" — revise and confirm before
collecting.

Collect all approved additions with their destinations. Do not apply yet.

## Step 5 — Strengthen violated rules

For each item in "Process violations", evaluate whether the existing rule's
language was clear enough. If the violation happened because the rule was
ambiguous or easy to overlook, propose a rewording.

Present each rewording proposal individually using AskUserQuestion with
destination routing (same options as Step 4 — the rule being strengthened
determines the destination).

Collect all approved rewordings with their destinations. Do not apply yet.

## Step 6 — Apply approved changes

Group all approved additions and rewordings by destination.

### Private destinations (1, 3, 5) — direct edits

For each private destination with approved changes:
1. Read the target file
2. Apply all approved additions and rewordings for that destination
3. Do not duplicate existing content

These are outside the repo — no commit needed.

### Repo destinations (2, 4) — commit via PR

For each repo destination with approved changes:
1. Read the target file in the worktree
2. Apply all approved additions and rewordings for that destination
3. Do not duplicate existing content

After all repo-destination edits are applied, commit once via `/flow:commit`.
Only CLAUDE.md and `.claude/` files are committed — never application code.

### Worktree memory rescue

If Source D contained items that were approved and routed to a destination,
those are already handled above. For any remaining useful items in the
worktree auto-memory that were not surfaced as proposals (e.g., structural
notes about the project that are clearly valuable), merge them into project
memory (destination 5: `~/.claude/projects/<repo-root>/memory/MEMORY.md`)
so they survive cleanup.

To determine `<repo-root>`: read `state["worktree"]`. The worktree is
inside the project (e.g., `/Users/ben/code/hh/.worktrees/my-feature`).
The repo root is the worktree's parent's parent — strip `.worktrees/<name>`
(e.g., `/Users/ben/code/hh`). Escape: replace `/` with `-`, drop the
leading `-`.

### Summary

Print a summary of what was written where:

````markdown
```text
============================================
  Reflect — Changes Applied
============================================

  Global CLAUDE.md: 2 additions
  Project rules (.claude/rules/testing.md): 1 addition
  Project memory: 3 items rescued from worktree
  Project CLAUDE.md: 1 addition (committed)

============================================
```
````

---

## Step 7 — Plugin improvement notes

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

Update Phase 8 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `9`

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.11.0 — Phase 8: Reflect — COMPLETE (<formatted_time>)
  Merge the PR, then run /flow:cleanup.
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 8: Reflect is complete. The PR now includes CLAUDE.md improvements. Ready to begin Phase 9: Cleanup?"
>
> - **Yes, start Phase 9 now** — invoke `flow:cleanup`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 9 now" and "Not yet"

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
- Always read all four sources before presenting findings
- Follow the reflection process (Steps 1 through 7) exactly — do not skip or reorder steps
- Plugin improvement notes are presented only — never committed
- Global writes (`~/.claude/CLAUDE.md`, `~/.claude/rules/`, `~/.claude/projects/`) are direct edits — never committed
- Repo writes (`CLAUDE.md`, `.claude/rules/`) go through `/flow:commit`
