---
title: "Phase 4: Simplify"
nav_order: 5
---

# Phase 4: Simplify

**Command:** `/flow-simplify`

Runs Claude Code's built-in `/simplify` skill on the PR diff. Refactors
code for clarity, reduces complexity, and improves naming while preserving
exact functionality. Safe because Phase 3 (Code) tests already verified
all behavior is preserved.

---

## What Simplify Does

The `/simplify` skill reviews recently changed code and fixes quality
issues automatically:

- Removes unnecessary abstractions and dead code
- Simplifies nested conditionals and reduces complexity
- Improves variable naming for clarity
- Consolidates duplicated patterns
- Enforces CLAUDE.md rules

It never changes **what** the code does, only **how** it does it.

---

## The Process

1. **Invoke `/simplify`** on the committed code from the Code phase
2. **Show the diff** — all proposed changes displayed inline
3. **User decides** — accept, revert, edit manually, or go back to Code
4. **Auto-commit** — accepted changes committed via `/flow-commit --auto`

---

## Back Navigation

- **Go back to Code** — revert simplifications and return to Code phase

---

## What Comes Next

Phase 5: Review (`/flow-review`) — systematic code review against the
plan, risks, and framework anti-patterns.
