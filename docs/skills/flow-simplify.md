---
title: /flow-simplify
nav_order: 8
parent: Skills
---

# /flow-simplify

**Phase:** 4 — Simplify

**Usage:** `/flow-simplify`, `/flow-simplify --auto`, or `/flow-simplify --manual`

Invokes Claude Code's built-in `/simplify` skill on the feature diff.
Refactors for clarity, reduces complexity, and improves naming while
preserving exact functionality. Auto-commits accepted changes before
transitioning to Review.

---

## Steps

1. Invoke `/simplify` on committed code
2. Show the diff for user review
3. User decides: accept, revert, edit, or go back to Code
4. Auto-commit accepted changes via `/flow-commit --auto`

---

## Mode

Mode is configurable via `.flow.json` (default: manual). In auto mode, refactoring is accepted without approval (diff is still shown) and the phase transition advances to Review without asking.

---

## Gates

- Code phase must be complete before Simplify can start
- Diff is always shown (in both modes)
- Can return to Code phase
