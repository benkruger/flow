---
title: /flow-issues
nav_order: 15
parent: Skills
---

# /flow-issues

**Phase:** Any

**Usage:**

```text
/flow-issues
/flow-issues --ready
/flow-issues --blocked
/flow-issues --decomposed
/flow-issues --quick-start
```

Fetches all open issues for the current repository, analyzes them via Python script (file paths, dependencies, labels, stale detection), ranks by impact using LLM judgment, and displays a dashboard with a recommended work order. Supports optional readiness filters to narrow results. Read-only — never creates, edits, or closes issues.

---

## What It Does

1. Runs `bin/flow analyze-issues` which calls `gh issue list` internally, parses the JSON, extracts file paths from issue bodies, detects `#N` dependency cross-references between open issues, detects "Flow In-Progress" and "decomposed" labels, categorizes issues, and checks for stale issues (older than 60 days with missing file references)
2. Reads the condensed per-issue briefs and ranks by impact using LLM judgment — considering what unblocks the most work, what has the broadest effect, and what is urgent
3. Displays a summary line with total issue count
4. Prints an In Progress table for WIP issues (linked `[#N](url)`, Title columns)
5. Prints a single Recommended Work Order table with columns: Order, Status, Impact, Labels, # (linked), Title, Rationale — excluding in-progress issues. Status shows `Ready` or `Blocked` based on the `dependencies` field. Ready issues appear first, blocked issues at the end. Sorting respects explicit dependency ordering (prerequisites before dependents) and impact ranking. A Start Commands table follows the work order with only ready issues — blocked issues are excluded from start commands

---

## Readiness Filters

Optional flags filter the issue list by readiness. Flags are mutually exclusive — pass at most one.

| Flag | Shows |
|------|-------|
| `--ready` | Issues with no dependencies — can start immediately |
| `--blocked` | Issues with unresolved dependencies — waiting on other work |
| `--decomposed` | Issues with the "Decomposed" label — work-ready with prior analysis |
| `--quick-start` | Decomposed issues with no dependencies — best candidates for autonomous execution |

No flag returns all issues (default behavior).

---

## Gates

- Read-only — never creates, edits, or closes issues
- Display-only — no AskUserQuestion prompts
