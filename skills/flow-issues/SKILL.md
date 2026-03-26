---
name: flow-issues
description: "Fetch open issues, analyze mechanically, rank by impact, and display a dashboard with recommended work order."
---

# FLOW Issues

Fetch all open issues for the current repository, analyze them via Python
script (file paths, dependencies, labels, stale detection), then rank by
impact using judgment and display a dashboard. Read-only — never create,
edit, or close issues.

## Usage

```text
/flow:flow-issues
/flow:flow-issues --ready
/flow:flow-issues --blocked
/flow:flow-issues --decomposed
/flow:flow-issues --quick-start
```

## Readiness Filters

Optional flags filter the issue list by readiness. Flags are mutually
exclusive — pass at most one.

- `--ready` — issues with no dependencies (can start immediately)
- `--blocked` — issues with unresolved dependencies (waiting on other work)
- `--decomposed` — issues with the "Decomposed" label (work-ready with prior analysis)
- `--quick-start` — decomposed issues with no dependencies (best candidates for autonomous execution)

No flag returns all issues (current default behavior).

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.0 — flow:flow-issues — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Fetch and Analyze

Run the analysis script, which calls `gh issue list` internally, parses the
JSON, extracts file paths, detects `#N` dependency cross-references between
open issues, detects "Flow In-Progress" and "decomposed" labels, checks for
stale issues (older than 60 days with missing file references), and outputs
condensed per-issue briefs:

If a readiness filter flag was passed to this skill, append it to the command:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --ready
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --blocked
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --decomposed
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --quick-start
```

Use the first form when no filter flag was passed. Use the matching form
when a flag was passed.

Parse the JSON output. The structure is:

```json
{
  "status": "ok",
  "total": 12,
  "in_progress": [{"number": 1, "title": "...", "url": "..."}],
  "issues": [
    {
      "number": 2,
      "title": "...",
      "url": "...",
      "labels": ["Decomposed"],
      "category": "Enhancement",
      "age_days": 5,
      "decomposed": true,
      "stale": false,
      "stale_missing": 0,
      "dependencies": [3],
      "dependents": [4],
      "file_paths": ["lib/foo.py"],
      "brief": "First ~200 chars of body..."
    }
  ]
}
```

If `status` is `"error"`, show the error message and stop.
If `total` is 0, print the COMPLETE banner and stop.

The `in_progress` array contains issues with the "Flow In-Progress" label —
these are being worked on by another engineer. The `issues` array contains
all other issues available for work.

## Step 2 — Rank by Impact

Read the condensed briefs from Step 1. For each issue, assess its impact
using your judgment — not a formula. Consider:

- **What would unblock the most work?** Issues with dependents block other
  issues from starting. Issues that are prerequisites for many others have
  high impact.
- **What has the broadest effect?** Issues touching many files or areas of
  the codebase have wider impact than narrowly scoped changes.
- **What is urgent?** Bugs, flaky tests, and issues blocking active work
  take priority over enhancements.
- **Is it ready for autonomous execution?** Decomposed issues are work-ready
  and can be started immediately without a planning phase.

Sort by highest impact, respecting dependency ordering: if issue A depends
on issue B (from the `dependencies` field), B must appear before A
regardless of impact assessment.

## Step 3 — Display

Print a summary line with the total issue count.

### In Progress Table

If the `in_progress` array is non-empty, print an "In Progress" table.
Columns: `#`, `Title`. The `#` column shows a markdown link: `[#N](url)`.

If no issues are in progress, skip this section.

### Recommended Work Order

Print a "Recommended Work Order" section as a single markdown table.
Columns: `Order`, `Impact`, `Labels`, `#`, `Title`, `Rationale`.

The `Impact` column shows your assessment: `High`, `Medium`, or `Low`.

The `#` column shows a markdown link: `[#N](url)`.

The `Labels` column shows the issue's labels as a comma-separated list.

For stale issues, append `[Stale: N files missing]` to the title where N
is the `stale_missing` count.

The `Rationale` column explains why this issue is at this position:

- If dependency-ordered: "prerequisite for #N" or "depends on #N"
- If decomposed: "decomposed — ready for autonomous execution"
- Otherwise: brief reason based on your impact assessment

### Start Commands

After the work order table, list a copy-paste start command for each issue:
`/flow:flow-start work on issue #N`

After the start commands are displayed, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.0 — flow:flow-issues — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Read-only — never create, edit, or close issues
- Display all open issues — in-progress issues appear in the In Progress table, all others in the work order table
- Exclude in-progress issues from the Recommended Work Order
- No AskUserQuestion — this is a display-only skill
- Never use Bash to print banners — output them as text in your response
