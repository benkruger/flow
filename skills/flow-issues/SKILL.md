---
name: flow-issues
description: "Group open issues by label into four sections (Blocked, Other, Vanilla, Decomposed) with mechanical sort and a copy-pasteable command per row."
---

# FLOW Issues

Fetch all open issues for the current repository, bucket them by label,
and render four tables. Read-only — never create, edit, or close issues.

## Usage

```text
/flow:flow-issues
/flow:flow-issues --ready
/flow:flow-issues --blocked
/flow:flow-issues --decomposed
/flow:flow-issues --quick-start
/flow:flow-issues --label Bug
/flow:flow-issues --label Bug --label "Tech Debt"
/flow:flow-issues --milestone v1.2
/flow:flow-issues --label Bug --ready
```

## Filter Flags

Filter flags shape which issues the Rust subcommand emits. Filtering
happens at the data layer — `bin/flow analyze-issues` returns a
pre-filtered `issues` array, and the renderer simply buckets and
renders whatever it receives. Flags are mutually exclusive within
each family.

- `--ready` — Rust drops blocked rows before delivery; no Blocked
  section appears in the output.
- `--blocked` — Rust keeps only blocked rows; only the Blocked
  section appears.
- `--decomposed` — Rust keeps only decomposed rows; only the
  Decomposed section appears.
- `--quick-start` — Rust keeps only decomposed, non-blocked,
  non-Flow-In-Progress rows; the Decomposed section renders with no
  🟡 cluster.
- `--label <name>` — server-side filter passed to `gh issue list`
  (repeatable; multiple labels combine with AND).
- `--milestone <title>` — server-side milestone filter
  (single value; by title or number).

`--label` and `--milestone` compose with the section flags. No flag
renders all four sections.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.2.0 — flow:flow-issues — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Fetch and Analyze

Run the analysis script. It calls `gh issue list` internally and emits
a single flat `issues` array with per-row label flags, assignees, and
URL-bearing `blocked_by` entries:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --ready
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --blocked
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --decomposed
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --quick-start
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --label Bug
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --label Bug --label "Tech Debt"
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --milestone v1.2
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow analyze-issues --label Bug --ready
```

Use the first form when no filter flag was passed. Use the matching form
when a flag was passed.

Parse the JSON output. The shape is:

```json
{
  "status": "ok",
  "total": 12,
  "issues": [
    {
      "number": 1547,
      "title": "...",
      "url": "https://github.com/owner/repo/issues/1547",
      "labels": ["Decomposed"],
      "decomposed": true,
      "blocked": false,
      "native_blocked": false,
      "blocked_by": [
        {"number": 1525, "url": "https://github.com/owner/repo/issues/1525"}
      ],
      "assignees": ["alice"],
      "vanilla": false,
      "flow_in_progress": false,
      "triage_in_progress": false
    }
  ]
}
```

If `status` is `"error"`, show the error message and stop.
If `total` is 0 AND a filter flag was passed (`--ready`,
`--blocked`, `--decomposed`, `--quick-start`, `--label`,
`--milestone`), print "No issues matched the filter — run
`/flow:flow-issues` without flags to see every open issue."
before the COMPLETE banner. If `total` is 0 with no filter flag,
print the COMPLETE banner and stop.

## Step 2 — Render the four sections

Render four markdown tables in order: **Blocked**, **Other**,
**Vanilla**, **Decomposed**. Each row belongs to exactly one section;
flags resolve membership and sort order.

### Bucket assignment

Walk the `issues` array once. For each row, assign to the first
section whose condition matches:

1. **Blocked** — `blocked == true` (label OR native_blocked).
2. **Decomposed** — `decomposed == true` AND `blocked == false`.
3. **Vanilla** — `vanilla == true` AND `decomposed == false` AND
   `blocked == false`.
4. **Other** — everything else.

The bucket assignment is independent of `flow_in_progress` and
`triage_in_progress` — in-progress signals are visual treatment
applied AFTER bucketing (see Color treatment below). A row that is
in-progress lands in whichever bucket its primary labels select; the
colored prefix and suppressed Command cell follow regardless of
which bucket received the row.

### Columns

The Blocked section renders five columns:

| Issue # | Title | Assignee | Blocked By | Command |
|---|---|---|---|---|

The Other, Vanilla, and Decomposed sections render four columns:

| Issue # | Title | Assignee | Command |
|---|---|---|---|

### Cell rules

- **Issue #** is `[#N](url)` — a markdown link to the issue. Always
  rendered.
- **Title** is the issue title. Bold (`**title**`) for rows where
  `flow_in_progress` or `triage_in_progress` is true; plain otherwise.
- **Assignee** is the first entry in `assignees`, or `—` when the array
  is empty. (Comma-separate additional logins if present.)
- **Blocked By** (Blocked section only) is a comma-separated list of
  `[#N](url)` entries from `blocked_by`, or `—` when `blocked_by` is
  empty but `blocked == true` (label-only block).
- **Command** depends on the bucket AND the in-progress signal.
  When `flow_in_progress == true` OR `triage_in_progress == true`,
  the Command cell renders `—` REGARDLESS of bucket — the colored
  prefix signals "someone else owns this" and the empty Command
  prevents a second engineer from firing a redundant slash command.
  Otherwise:
  - Blocked section: `—`.
  - Other section: ```/flow:flow-explore work on issue #N```
  - Vanilla section: ```/flow:flow-plan #N```
  - Decomposed section: ```/flow:flow-start #N```
- **Empty-cell convention.** Every empty cell renders as `—`.
- **Markdown safety.** Issue titles and assignee logins flow from
  GitHub unescaped. Before rendering, escape `|`, `\`, `\n`, `\r`
  in every Title and Assignee cell (replace `|` with `\|`, `\` with
  `\\`, newlines and carriage returns with spaces). Never render
  HTML from titles — treat angle brackets, `[`, `]`, `(`, `)` as
  literal characters by wrapping the cell content in backticks for
  any title that contains them. The same escaping applies to
  Blocked-By URL link text. Per
  `.claude/rules/subprocess-argument-escaping.md`, external data
  must be escaped at the rendering boundary; an unescaped pipe in
  a title breaks the table for every downstream row, and an
  unescaped image tag in a title can exfiltrate the viewer's
  request to a third-party server.

### Color treatment

Rows carrying the canonical FLOW labels get visual treatment that
applies regardless of bucket:

- `flow_in_progress == true` (Flow In-Progress label) → 🟡 prefix
  on the bold Title cell, Command suppressed.
- `triage_in_progress == true` (Triage In-Progress label) → 🔍
  prefix on the bold Title cell, Command suppressed.
- `high_priority == true` (High Priority label) → 🔥 prefix on the
  Title cell. 🔥 does NOT bold the Title, does NOT suppress the
  Command cell, and applies regardless of bucket — it is additive,
  not exclusive with the other prefixes.

The prefix follows the row into whichever bucket it lands; a
Flow-In-Progress row in the Vanilla bucket still renders 🟡, a
Triage-In-Progress row in the Blocked bucket still renders 🔍, and
a High-Priority row in any bucket still renders 🔥. The
cross-engineer WIP signal documented in `CLAUDE.md` "The 'Flow
In-Progress' label on issues is the cross-engineer WIP detection
mechanism" is honored from every section.

When 🔥 stacks with 🟡 or 🔍 on the same row, 🔥 leads:
`🔥 🟡 **Title**` or `🔥 🔍 **Title**`. The bolding and Command
suppression still come from the in-progress signal; 🔥 is additive
and does not change either behavior.

### Sort rules

- **Blocked** and **Vanilla** sections: sort by issue `number`
  descending (newest issue numbers first).
- **Other** and **Decomposed** sections: sort colored rows first
  (Decomposed section: 🟡 rows; Other section: 🔍 rows), then by issue
  `number` descending within each cluster.
- `high_priority` does not participate in sort clustering; 🔥 rows
  stay in their bucket's normal number-descending order.

### Filter flag effect

Filtering happens in `bin/flow analyze-issues` at the Rust layer,
not at the rendering layer — the `issues` array delivered to the
renderer already reflects the active filter. Empty sections do not
render. The user-facing effect of each flag:

- No flag → all four sections in order.
- `--ready` → Blocked section absent (Rust dropped blocked rows).
- `--blocked` → only Blocked section appears.
- `--decomposed` → only Decomposed section appears.
- `--quick-start` → only Decomposed section appears, no 🟡 cluster
  (Rust dropped both blocked AND flow_in_progress rows before
  delivery).
- `--label` / `--milestone` → whichever sections the surviving
  rows populate.

After the sections are rendered, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.2.0 — flow:flow-issues — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Read-only — never create, edit, or close issues.
- Bucketing and sort are mechanical — no LLM judgment.
- Colored rows are visual-only; the Command cell stays suppressed per
  the bucket rules so the row signals "someone else owns this".
- No AskUserQuestion — this is a display-only skill.
- Never use Bash to print banners — output them as text in your response.
