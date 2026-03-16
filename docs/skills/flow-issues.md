---
title: /flow-issues
nav_order: 15
parent: Skills
---

# /flow-issues

**Phase:** Any

**Usage:** `/flow-issues`

Fetches all open issues for the current repository, categorizes them, prioritizes within each category, detects batchable issues, and displays a dashboard with a recommended work order. Read-only — never creates, edits, or closes issues.

---

## What It Does

1. Runs `gh issue list` to fetch all open issues (up to 100)
2. Categorizes each issue using label-based categories first (Rule, Flow, Flaky Test, Tech Debt, Documentation Drift), then content-based fallbacks (Bug, Enhancement, Other)
3. Prioritizes within each category: High, Medium, or Low based on age and impact
4. Detects batchable issues by scanning bodies for shared file paths (2+ shared files groups issues into a batch via transitive closure)
5. Displays a summary line with total and per-category counts
6. Prints a markdown table per category sorted by priority then age
7. Prints a Recommended Work Order accounting for priority, batches, and file-level dependencies

---

## Gates

- Read-only — never creates, edits, or closes issues
- Display-only — no AskUserQuestion prompts
