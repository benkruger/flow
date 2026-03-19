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
4. Prioritizes within each category: High, Medium, or Low based on age and impact
5. Analyzes issues across four dimensions:
   - **Batch detection** — scans bodies for shared file paths (2+ shared files groups issues into a batch via transitive closure)
   - **Dependency detection** — scans bodies for `#N` cross-references to build an explicit dependency graph between open issues
   - **File count** — counts file path references per issue as a complexity signal
   - **Stale detection** — for issues older than 60 days, checks whether referenced files still exist via Glob and flags missing files
6. Displays a summary line with total and per-category counts, plus an "In Progress" section for WIP issues
7. Prints a markdown table per category with columns: #, Title, Files, Age, Priority — sorted by priority then age. Annotations: `[In Progress]`, `[Decomposed]`, `[Stale: N files missing]`
8. Prints a Recommended Work Order excluding in-progress issues. Explicit dependencies (from `#N` cross-references) are resolved first via topological sort, then priority, batches, implicit dependencies, decomposed boost, and age. Each entry includes a copy-paste `/flow:flow-start` command

---

## Gates

- Read-only — never creates, edits, or closes issues
- Display-only — no AskUserQuestion prompts
