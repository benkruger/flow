---
name: flow-issues
description: "Fetch open issues, categorize, prioritize, detect batchable issues, and display a dashboard with recommended work order."
---

# FLOW Issues

Fetch all open issues for the current repository, categorize them, prioritize within each category, and display a dashboard. Read-only — never create, edit, or close issues.

## Usage

```text
/flow:flow-issues
```

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
  FLOW v0.33.1 — flow:flow-issues — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Fetch

Run:

```bash
gh issue list --state open --json number,title,labels,createdAt,body --limit 100
```

Parse the JSON output. If there are no open issues, print the COMPLETE banner and stop.

## Step 2 — Detect In-Progress Issues

Check each issue's labels for "Flow In-Progress". Issues with this label
are actively being worked on by another FLOW feature (possibly on a
different engineer's machine). Mark these issues as in-progress for use
in later steps. Still include them in categorization and display — the
label is an annotation, not a filter.

## Step 3 — Categorize

Assign each issue to exactly one category. If an issue has a label
matching one of the label-based categories below, use that label as
the category directly. Otherwise, fall back to content analysis of
the title and body:

**Label-based categories** (matched by GitHub label):

- **Rule** — rule addition or update for `.claude/rules/`
- **Flow** — FLOW process gap or improvement
- **Flaky Test** — intermittent test failure with reproduction data
- **Tech Debt** — working but fragile, duplicated, or convention-violating code
- **Documentation Drift** — docs out of sync with actual behavior

**Content-based categories** (fallback when no label matches):

- **Bug** — something is broken or behaving incorrectly
- **Enhancement** — new feature or improvement to existing behavior
- **Other** — does not fit any category above

## Step 4 — Prioritize

Within each category, assign High, Medium, or Low priority based on:

- **High** — older than 30 days, blocks workflow, or affects correctness
- **Medium** — older than 7 days, or affects developer experience
- **Low** — recent, cosmetic, or nice-to-have

## Step 5 — Batch Detection

Scan each issue's body for file path references. File paths are strings
containing `/` with recognizable patterns: directory prefixes like `lib/`,
`skills/`, `tests/`, `docs/`, `hooks/`, `frameworks/`, `.claude/`, or
paths ending with file extensions like `.py`, `.md`, `.json`, `.sh`.

For each pair of issues, check whether they share 2 or more file paths.
Group issues that share files using transitive closure: if issue A shares
files with B, and B shares files with C, then A, B, and C form one batch.

Record:

- **Batches** — groups of 2+ issues with their shared file paths
- **Solo issues** — issues that do not share 2+ files with any other issue

If no batches are found (all issues are solo), skip the batch output in
the next step.

## Step 6 — Display

Print a summary line with total count and per-category counts.

### In Progress Section

If any issues have the "Flow In-Progress" label, print an "In Progress"
section before the category tables listing each in-progress issue with
its number and title. If no issues have the label, skip this section.

### Category Tables

For each non-empty category, print a markdown table with columns: `#`, `Title`, `Age`, `Priority`. Sort by priority (High first), then by age (oldest first).

For in-progress issues, append `[In Progress]` to the title in the table.
Never remove in-progress issues from the table — always display all issues.

### Recommended Work Order

After the category tables, print a "Recommended Work Order" section.
This is a numbered list showing the recommended sequence for working
through the issues. Exclude issues with the "Flow In-Progress" label
from the work order — they are already being worked on by another
engineer.

- **Priority ordering** — High before Medium before Low
- **Batches as units** — when issues form a batch, list them as a group.
  The batch's effective priority is its highest-priority member.
- **Dependencies** — if one issue refactors files that another issue
  adds features to (based on category: Tech Debt, Rule, or Flow issues
  that touch shared files), place the refactoring issue first
- **Ties** — broken by age (oldest first)

For each entry in the work order, show:

- Issue number(s) and title(s)
- Effective priority
- If batched: the shared files that link them
- If dependency-ordered: brief rationale

If there are no batches and no dependency relationships, the work order
is simply the priority-then-age sort from the category tables. State
this rather than repeating the full list.

After the work order is displayed, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.33.1 — flow:flow-issues — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Read-only — never create, edit, or close issues
- Display all open issues in category tables — annotate in-progress issues, never remove rows
- Exclude in-progress issues from the Recommended Work Order
- No AskUserQuestion — this is a display-only skill
- Never use Bash to print banners — output them as text in your response
