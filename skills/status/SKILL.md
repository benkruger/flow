---
name: status
description: "Show current ROR phase, PR link, phase checklist, and what comes next. Rebuilds the task list from .claude/ror-state.json. Use any time you want to know where you are in the workflow."
---

# ROR Status

Show where you are in the ROR workflow and rebuild the task list from persisted state.

## Announce

Print:

```
============================================
  ROR — ror:status — STARTING
============================================
```

## Steps

### Step 1 — Read the state file

Read `.claude/ror-state.json` from the project root.

If the file does not exist, report:
```
No ROR feature in progress. Start one with /ror:start <feature name>.
```
Then stop.

### Step 2 — Rebuild the task list

For each phase in the state file, call TaskCreate with:
- Subject: `Phase <number>: <name>`
- Status based on the phase's `status` field:
  - `complete` → mark task as completed immediately after creating
  - `in_progress` → mark task as in_progress
  - `pending` → leave as pending

This replaces any stale task list from a previous session.

### Step 3 — Print status panel

```
============================================
  ROR — Current Status
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

  Next: /ror:research

============================================
```

Use `[x]` for complete, `[>]` for in_progress, `[ ]` for pending.

### Step 4 — If all phases complete

If all phases show `complete`, print:

```
============================================
  ROR — All phases complete!
  Feature: <feature>
  This feature is fully done.
============================================
```

## Rules

- Never modify the state file or any other files — this skill is read-only
- Always rebuild the full task list, not just the current phase
