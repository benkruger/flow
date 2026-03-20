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

1. Runs `gh issue list` to fetch all open issues (up to 100) including URL for linking
2. Detects issues with the "Flow In-Progress" and "decomposed" labels — in-progress issues are being worked on by another FLOW feature; decomposed issues were filed via `/create-issue` with DAG analysis and are work-ready for autonomous execution
3. Categorizes each issue using label-based categories first (Rule, Flow, Flaky Test, Tech Debt, Documentation Drift), then content-based fallbacks (Bug, Enhancement, Other)
4. Analyzes issues across six dimensions:
   - **Batch detection** — scans bodies for shared file paths (2+ shared files groups issues into a batch via transitive closure)
   - **Dependency detection** — scans bodies for `#N` cross-references to build an explicit dependency graph between open issues
   - **File count** — counts file path references per issue for batch detection
   - **Stale detection** — for issues older than 60 days, checks whether referenced files still exist via Glob and flags missing files
   - **Impact analysis** — scores each issue on cross-area scope, force-multiplier language, acceptance criteria density, and reverse reference count (4 signals, tier: High/Medium/Low)
   - **Blocking score** — computes reverse dependency count per issue to identify blockers
5. Prioritizes using category-based default tiers (Bug/Flaky Test=High, Tech Debt/Rule/Flow/Documentation Drift=Medium, Enhancement/Other=Low), then applies impact and blocking modifiers that can promote one tier each — age is only a tiebreaker within the same tier
6. Displays a summary line with total and per-category counts
7. Prints an In Progress table for WIP issues (linked `[#N](url)`, Title columns)
8. Prints a single Recommended Work Order table with columns: Order, Priority, Impact, Labels, # (linked), Title, Rationale — excluding in-progress issues. Sorting: explicit dependencies (topological sort) → priority → batches → implicit dependencies → decomposed boost → age. Each entry gets a copy-paste `/flow:flow-start` command listed after the table

---

## Gates

- Read-only — never creates, edits, or closes issues
- Display-only — no AskUserQuestion prompts
