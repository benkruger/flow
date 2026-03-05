---
title: /flow:review
nav_order: 8
parent: Skills
---

# /flow:review

**Phase:** 5 — Review

**Usage:** `/flow:review`

Systematic review against design, research risks, and framework
anti-patterns. Fixes findings, runs `bin/flow ci` after every fix, then
transitions to Security.

---

## What It Checks

| Area | What |
|------|------|
| Design alignment | Implementation matches `state["design"]` |
| Research risks | Every risk in `state["research"]["risks"]` accounted for |
| Anti-patterns | Framework-specific checks from the skill's framework instructions |
| Tests | Test infrastructure used correctly, both branches covered, assertions present |
| Clarity | Descriptive names, no inline comments, no over-engineering |

---

## Fixing Findings

- Minor → fix directly, commit, re-run `bin/flow ci`
- Significant → AskUserQuestion: fix here or go back to Code/Plan/Design/Research

---

## Gates

- `bin/flow ci` must be green after every fix
- `bin/flow ci` must be green before transitioning to Security
- Full diff must be read before review begins
- Can return to Code or Plan
