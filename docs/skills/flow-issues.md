---
title: /flow-issues
nav_order: 15
parent: Skills
---

# /flow-issues

**Phase:** Any

**Usage:** `/flow-issues`

Fetches all open issues for the current repository, categorizes them, prioritizes within each category, and displays a dashboard. Read-only — never creates, edits, or closes issues.

---

## What It Does

1. Runs `gh issue list` to fetch all open issues (up to 100)
2. Categorizes each issue using label-based categories first (Rule, Flow, Flaky Test, Tech Debt, Documentation Drift), then content-based fallbacks (Bug, Enhancement, Other)
3. Prioritizes within each category: High, Medium, or Low based on age and impact
4. Displays a summary line with total and per-category counts
5. Prints a markdown table per category sorted by priority then age

---

## Gates

- Read-only — never creates, edits, or closes issues
- Display-only — no AskUserQuestion prompts
