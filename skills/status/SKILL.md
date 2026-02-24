---
name: status
description: "Show current SDLC phase, PR link, timing, and what comes next. Reads .claude/flow-states/<branch>.json. Use any time you want to know where you are in the workflow."
---

# FLOW Status

Show where you are in the FLOW workflow. Reads the state file and
prints a status panel. Read-only — never modifies anything.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — flow:status — STARTING
============================================
```
````

## Steps

### Step 1 — Read the state file

Find the project root and read `.claude/flow-states/<branch>.json`.

If no state file exists for the current branch, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
No FLOW feature in progress on this branch.
Start one with /flow:start <feature name>.
```
````

Then stop.

### Step 2 — Print status panel

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW v0.6.2 — Current Status
============================================

  Feature : <feature>
  Branch  : <branch>
  PR      : <pr_url>

  Phases
  ------
  [x] Phase 1:  Start
  [>] Phase 2:  Research   <-- YOU ARE HERE
  [ ] Phase 3:  Design
  [ ] Phase 4:  Plan
  [ ] Phase 5:  Code
  [ ] Phase 6:  Review
  [ ] Phase 7:  Reflect
  [ ] Phase 8:  Cleanup

  Time in current phase : <cumulative_seconds formatted as Xh Ym>
  Times visited         : <visit_count>

  Next: /flow:research

============================================
```
````

Use `[x]` for complete, `[>]` for in_progress, `[ ]` for pending.

If all phases are complete, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — All phases complete!
  Feature: <feature>
  This feature is fully done.
============================================
```
````

## Rules

- Read-only — never modifies the state file or any other files
- Never calls TaskCreate or TaskUpdate