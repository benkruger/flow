---
title: /sdlc:status
nav_order: 3
parent: Skills
---

# /sdlc:status

**Phase:** Any

**Usage:** `/sdlc:status`

Shows where you are in the SDLC workflow at any moment. Reads the PR checklist and prints a clear picture of what has been completed and what comes next.

---

## What It Does

1. Finds the open PR for the current branch
2. Reads the phase checklist from the PR body
3. Identifies completed phases, remaining phases, and the current phase
4. Prints a status panel with the next command to run

---

## Example Output

```
============================================
  SDLC — Current Status
============================================

  Feature : App Payment Webhooks
  Branch  : app-payment-webhooks
  PR      : https://github.com/org/repo/pull/42

  Phases
  ------
  [x] Phase 0: Start
  [ ] Phase 1: Research   <-- YOU ARE HERE
  [ ] Phase 2: Design
  [ ] Phase 3: Plan
  [ ] Phase 4: Implement
  [ ] Phase 5: Test
  [ ] Phase 6: Review
  [ ] Phase 7: Ship

  Next: /sdlc:research

============================================
```

---

## Gates

- Read-only — never modifies the PR or any files
- Reports clearly if no PR is found or the checklist is missing
