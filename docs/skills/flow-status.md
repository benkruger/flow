---
title: /flow-status
nav_order: 3
parent: Skills
---

# /flow-status

**Phase:** Any

**Usage:** `/flow-status`

Shows where you are in the FLOW workflow at any moment. Reads `.flow-states/<branch>.json` and prints a clear picture of what has been completed and what comes next. Read-only — never modifies any files.

---

## What It Does

1. Reads `.flow-states/<branch>.json` from the project root
2. Prints a status panel with current phase, timing, and next command

---

## Example Output

```text
============================================
  FLOW v0.8.4 — Current Status
============================================

  Feature : App Payment Webhooks
  Branch  : app-payment-webhooks
  PR      : https://github.com/org/repo/pull/42
  Elapsed : 1h 15m
  Notes   : 2
  Tasks   : 3/7 complete

  Phases
  ------
  [x] Phase 1:  Start          (<1m)
  [x] Phase 2:  Plan           (15m)
  [>] Phase 3:  Code           <-- YOU ARE HERE
  [ ] Phase 4:  Code Review
  [ ] Phase 5:  Learn
  [ ] Phase 6:  Cleanup

  Time in current phase : 32m
  Times visited         : 1

  Continue: /flow-code

============================================
```

---

## Gates

- Read-only — never modifies any files
- Reports clearly if no state file is found for the current branch
