---
name: flow-issues
description: "Fetch open issues, categorize, prioritize, and display a dashboard."
model: haiku
---

# FLOW Issues

Fetch all open issues for the current repository, categorize them, prioritize within each category, and display a dashboard. Read-only — never create, edit, or close issues.

## Usage

```text
/flow:flow-issues
```

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.14 — flow:flow-issues — STARTING
============================================
```
````

## Step 1 — Fetch

Run:

```bash
gh issue list --state open --json number,title,labels,createdAt,body --limit 100
```

Parse the JSON output. If there are no open issues, print the COMPLETE banner and stop.

## Step 2 — Categorize

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

## Step 3 — Prioritize

Within each category, assign High, Medium, or Low priority based on:

- **High** — older than 30 days, blocks workflow, or affects correctness
- **Medium** — older than 7 days, or affects developer experience
- **Low** — recent, cosmetic, or nice-to-have

## Step 4 — Display

Print a summary line with total count and per-category counts.

Then for each non-empty category, print a markdown table with columns: `#`, `Title`, `Age`, `Priority`. Sort by priority (High first), then by age (oldest first).

After all categories are displayed, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.14 — flow:flow-issues — COMPLETE
============================================
```
````

## Hard Rules

- Read-only — never create, edit, or close issues
- Display all open issues — never filter or hide
- No AskUserQuestion — this is a display-only skill
- Never use Bash to print banners — output them as text in your response
