---
name: reflect
description: "Phase 7: Reflect — review what went wrong, capture learnings in CLAUDE.md, note plugin improvements. Runs before the PR is merged. The only commits are CLAUDE.md and .claude/ changes."
---

# FLOW Reflect — Phase 7: Reflect

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.claude/flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
4. Check `phases.6.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 6: Review must be
     complete. Run /flow:review first."
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — Phase 7: Reflect — STARTING
  Recommended model: Sonnet
============================================
```
````

## Update State

Read `.claude/flow-states/<branch>.json`. cd into the worktree.

Update Phase 7:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `7`

## Logging

Append a completion timestamp to every Bash command in the **same Bash call**:

```bash
COMMAND; EC=$?; date -u +"%Y-%m-%dT%H:%M:%SZ [Phase 7] Step X — desc (exit $EC)" >> .claude/flow-states/<branch>.log; exit $EC
```

Get `<branch>` from the state file. The gap between consecutive entries
is Claude's processing + command time.

---

## Step 1 — Gather all sources

Read and synthesise from three sources before asking the user anything:

### Source A — State file data

For each phase, note:
- `visit_count` > 1 → this phase had friction, was revisited
- `cumulative_seconds` unusually high → this phase took much longer than expected
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

With the evidence gathered in Step 1, follow the shared reflection
process in `docs/reflection-process.md` (Steps A through E).

When Step E says to commit, use `/flow:commit`. The commit goes onto the
feature branch so CLAUDE.md improvements merge to main with the feature.

---

## Step 3 — Plugin improvement notes

Present the plugin gaps inside a fenced code block:

````
```
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
> - **No, that's everything**
> - **Yes, add this** — describe in Other

---

## Done — Update state and complete phase

Update Phase 7 in state:
1. `cumulative_seconds` += `current_time - session_started_at`
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `8`

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 7: Reflect is complete. The PR now includes CLAUDE.md improvements. Ready to begin Phase 8: Cleanup?"
> - **Yes, start Phase 8 now** — invoke `flow:cleanup`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 8 now" and "Not yet"

**If Yes**, print inside a fenced code block:

````
```
============================================
  FLOW — Phase 7: Reflect — COMPLETE
  Merge the PR, then run /flow:cleanup.
============================================
```
````

**If Not yet**, print inside a fenced code block:

````
```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```
````

---

## Hard Rules

- Never commit application code in Reflect — only CLAUDE.md and .claude/
- Always read all three sources before presenting findings
- Follow `docs/reflection-process.md` exactly — do not skip or reorder steps
- Plugin improvement notes are presented only — never committed