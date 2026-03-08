---
title: /flow-review
nav_order: 8
parent: Skills
---

# /flow-review

**Phase:** 5 — Review

**Usage:** `/flow-review`, `/flow-review --auto`, or `/flow-review --manual`

Delegates to Claude's built-in `/review` command for code quality,
correctness, security, and test coverage analysis. Fixes every finding,
runs `bin/flow ci` after every fix, then transitions to Security.

---

## Mode

Mode is configurable via `.flow.json` (default: manual). In auto mode, significant findings are auto-fixed here (no user routing choice) and the phase transition advances to Security without asking.

---

## Fixing Findings

- Minor → fix directly, commit, re-run `bin/flow ci`
- Significant (manual mode) → AskUserQuestion: fix here or go back to Code/Plan
- Significant (auto mode) → fix directly here in Review

---

## Gates

- `bin/flow ci` must be green after every fix
- `bin/flow ci` must be green before transitioning to Security
- Full diff must be read before review begins
- Can return to Code or Plan
