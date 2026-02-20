---
title: /flow:status
nav_order: 3
parent: Skills
---

# /flow:status

**Phase:** Any

**Usage:** `/flow:status`

Shows where you are in the FLOW workflow at any moment. Reads `.claude/flow-states/<branch>.json` and prints a clear picture of what has been completed and what comes next. Rebuilds the Claude task list from persisted state.

---

## What It Does

1. Reads `.claude/flow-states/<branch>.json` from the project root
2. Rebuilds the task list using TaskCreate for each phase
3. Prints a status panel with current phase, timing, and next command

---

## Example Output

```
============================================
  FLOW — Current Status
============================================

  Feature : App Payment Webhooks
  Branch  : app-payment-webhooks
  PR      : https://github.com/org/repo/pull/42

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

  Time in current phase : 32m
  Times visited         : 1

  Next: /flow:research

============================================
```

---

## Gates

- Read-only — never modifies any files
- Reports clearly if no state file is found for the current branch
