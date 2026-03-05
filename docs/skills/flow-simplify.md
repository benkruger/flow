---
title: /flow:simplify
nav_order: 8
parent: Skills
---

# /flow:simplify

**Phase:** 4 — Simplify

**Usage:** `/flow:simplify`

Invokes Claude Code's built-in `/simplify` skill on the feature diff.
Refactors for clarity, reduces complexity, and improves naming while
preserving exact functionality. Auto-commits accepted changes before
transitioning to Review.

---

## Steps

1. Invoke `/simplify` on committed code
2. Show the diff for user review
3. User decides: accept, revert, edit, or go back to Code
4. Auto-commit accepted changes via `/flow:commit --auto`

---

## Gates

- Code phase must be complete before Simplify can start
- User must approve or reject the simplifications before proceeding
- Can return to Code phase
