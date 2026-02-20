---
name: status
description: "Show current SDLC phase, PR link, timing, and what comes next. Reads .claude/sdlc-states/<branch>.json. Use any time you want to know where you are in the workflow."
---

# SDLC Status

Show where you are in the SDLC workflow. Reads the state file and
prints a status panel. Read-only — never modifies anything.

## Announce

Print:

```
============================================
  SDLC — sdlc:status — STARTING
============================================
```

## Steps

### Step 1 — Read the state file

Find the project root and read `.claude/sdlc-states/<branch>.json`.

If no state file exists for the current branch, report:
```
No SDLC feature in progress on this branch.
Start one with /sdlc:start <feature name>.
```
Then stop.

### Step 2 — Print status panel

```
============================================
  SDLC — Current Status
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
  [ ] Phase 5:  Implement
  [ ] Phase 6:  Test
  [ ] Phase 7:  Review
  [ ] Phase 8:  Ship
  [ ] Phase 9:  Reflect
  [ ] Phase 10: Cleanup

  Time in current phase : <cumulative_seconds formatted as Xh Ym>
  Times visited         : <visit_count>

  Next: /sdlc:research

============================================
```

Use `[x]` for complete, `[>]` for in_progress, `[ ]` for pending.

If all phases are complete, print:

```
============================================
  SDLC — All phases complete!
  Feature: <feature>
  This feature is fully done.
============================================
```

## Rules

- Read-only — never modifies the state file or any other files
- Never calls TaskCreate or TaskUpdate
