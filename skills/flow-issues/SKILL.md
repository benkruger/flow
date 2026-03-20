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
  FLOW v0.36.1 — flow:flow-issues — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Fetch

Run:

```bash
gh issue list --state open --json number,title,labels,createdAt,body,url --limit 100
```

Parse the JSON output. If there are no open issues, print the COMPLETE banner and stop.

## Step 2 — Detect In-Progress and Decomposed Issues

Check each issue's labels for "Flow In-Progress". Issues with this label
are actively being worked on by another FLOW feature (possibly on a
different engineer's machine). Mark these issues as in-progress for use
in later steps. Still include them in categorization and display — the
label is an annotation, not a filter.

Also check each issue's labels for "decomposed". Issues with this label
were filed via `/create-issue` with DAG analysis and deep codebase
exploration — they are work-ready for autonomous execution. Mark these
issues as decomposed for use in later steps. Still include them in
categorization and display — the label is an annotation, not a filter.

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

## Step 4 — Batch Detection and Analysis

### 4a. Batch Detection

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
- **File count** — the number of file path references per issue, used for
  batch detection

If no batches are found (all issues are solo), omit the batch section
from the Step 6 display.

### 4b. Dependency Detection

Scan each issue's body for `#N` patterns where N matches the number of
another open issue in the fetched list. If issue A's body mentions `#B`,
then A depends on B — B must be completed before A.

Build a dependency map recording which issues depend on which. Only
record dependencies between issues that are both in the current open
issue list — references to closed or non-existent issues are ignored.

### 4c. Stale Detection

For issues older than 60 days that have file path references and are not
marked in-progress, check whether each referenced file still exists using
the Glob tool. Run Glob calls for all qualifying issues in parallel.
Count the number of missing files per issue. Mark issues with one or more
missing files as stale. Record the missing file count for display in
Step 6.

### 4d. Impact Analysis

For each issue, compute an impact score from four signals:

- **Cross-area scope** — count distinct top-level directories (`lib/`,
  `skills/`, `hooks/`, `tests/`, `docs/`, `frameworks/`, `.claude/`)
  from file paths already extracted in 4a. 4 or more areas = 1 point.
- **Force-multiplier language** — scan the body for keywords: "automate",
  "batch processing", "overnight", "processes issues", "all issues",
  "each issue", "enables", "force multiplier", "unattended". Any match
  = 1 point.
- **Acceptance criteria density** — count `- [ ]` patterns in the body.
  10 or more = 1 point.
- **Reverse reference count** — from the dependency map built in 4b,
  count how many other open issues depend on this issue. 2 or more
  = 1 point.

Impact tier: 3-4 points = High, 1-2 points = Medium, 0 points = Low.

### 4e. Blocking Score

Using the reverse reference count from 4d, an issue with 1 or more
dependents triggers the blocking modifier in Step 5.

## Step 5 — Prioritize

Assign each issue a priority tier using category-based defaults, then
apply impact and blocking modifiers.

### Default tier by category

- **High** — Bug, Flaky Test
- **Medium** — Tech Debt, Rule, Flow, Documentation Drift
- **Low** — Enhancement, Other

### Modifiers

- **Impact modifier** — if the impact tier from 4d is High, promote
  the issue one tier (Low → Medium, Medium → High, High stays High).
- **Blocking modifier** — if the issue blocks 1 or more other open
  issues (from 4e), promote one tier.
- Modifiers stack: an issue at Low with both High impact and blocking
  promotes to High (Low → Medium → High).

### Tiebreaking

- Decomposed issues sort before non-decomposed within the same tier.
- Age is used only as a tiebreaker within the same tier — it never
  promotes across tiers.

## Step 6 — Display

Print a summary line with total count and per-category counts.

### In Progress Table

If any issues have the "Flow In-Progress" label, print an "In Progress"
table before the work order. Columns: `#`, `Title`. The `#` column
shows a markdown link: `[#N](issue_url)`. If no issues have the label,
skip this section.

### Recommended Work Order

Print a "Recommended Work Order" section as a single markdown table.
Columns: `Order`, `Priority`, `Labels`, `#`, `Title`, `Rationale`.

The `#` column shows a markdown link: `[#N](issue_url)`.

The `Labels` column shows the issue's actual GitHub labels as a
comma-separated list.

Exclude issues with the "Flow In-Progress" label from the work order —
they are already being worked on by another engineer and appear only in
the In Progress table above.

For stale issues (from Step 4c), append `[Stale: N files missing]` to
the title where N is the count of missing files.

**Sorting algorithm** (determines the `Order` column):

- **Explicit dependencies first** — if issue A depends on B (from Step 4b),
  B must appear before A regardless of priority. Use topological sort to
  respect the full dependency chain. If a cycle exists, note it and fall
  back to priority ordering for the cycle members.
- **Priority ordering** — High before Medium before Low
- **Batches as units** — when issues form a batch, list them as a group.
  The batch's effective priority is its highest-priority member.
- **Implicit dependencies** — if one issue refactors files that another issue
  adds features to (based on category: Tech Debt, Rule, or Flow issues
  that touch shared files), place the refactoring issue first
- **Decomposed boost** — issues with the "decomposed" label sort before
  non-decomposed issues at the same priority/batch/dependency level. For
  batches, a batch containing any decomposed member is treated as decomposed.
- **Ties** — broken by age (oldest first)

The `Rationale` column explains why this issue is at this position:

- If dependency-ordered: "prerequisite for #N" or "depends on #N"
- If batched: "batch with #N (shared: file1, file2)"
- If decomposed: "decomposed — ready for autonomous execution"
- Otherwise: the priority tier and category

If batches exist, add a note above or below the table listing each
batch and the shared files that link its members.

### Start Commands

After the work order table, list a copy-paste start command for each
entry: `/flow:flow-start work on issue #N`

After the start commands are displayed, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.36.1 — flow:flow-issues — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Read-only — never create, edit, or close issues
- Display all open issues — in-progress issues appear in the In Progress table, all others in the work order table
- Exclude in-progress issues from the Recommended Work Order
- No AskUserQuestion — this is a display-only skill
- Never use Bash to print banners — output them as text in your response
