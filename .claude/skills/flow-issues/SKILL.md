---
name: flow-issues
description: "Fetch open issues, categorize, prioritize, and display a dashboard."
---

# FLOW Issues

Fetch all open issues for the FLOW repo, categorize them, prioritize within each category, and display a dashboard. Read-only — never create, edit, or close issues.

## Step 1 — Fetch

Run:

```bash
gh issue list --repo benkruger/flow --state open --json number,title,labels,createdAt,body --limit 100
```

Parse the JSON output. If there are no open issues, print the COMPLETE banner and stop.

## Step 2 — Categorize

Assign each issue to exactly one category. Use labels first, then fall back to content analysis of the title and body:

- **Bug** — something is broken or behaving incorrectly
- **Enhancement** — new feature or improvement to existing behavior
- **Learning** — insight, pattern, or convention to capture
- **Process Gap** — missing workflow step, skill gap, or automation opportunity
- **Documentation** — docs out of sync, missing docs, or unclear docs
- **Other** — does not fit any category above

## Step 3 — Prioritize

Within each category, assign High, Medium, or Low priority based on:

- **High** — older than 30 days, blocks workflow, or affects correctness
- **Medium** — older than 7 days, or affects developer experience
- **Low** — recent, cosmetic, or nice-to-have

## Step 4 — Display

Print the announce banner:

````markdown
```text
============================================
  Issues — STARTING
============================================
```
````

Print a summary line with total count and per-category counts.

Then for each non-empty category, print a markdown table with columns: `#`, `Title`, `Age`, `Priority`. Sort by priority (High first), then by age (oldest first).

After all categories are displayed, print:

````markdown
```text
============================================
  Issues — COMPLETE
============================================
```
````

## Rules

- Read-only — never create, edit, or close issues
- Display all open issues — never filter or hide
- No AskUserQuestion — this is a display-only skill
