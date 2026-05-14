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
/flow-issues --label Bug
/flow-issues --label Bug --label "Tech Debt"
/flow-issues --milestone v1.2
/flow-issues --label Bug --ready
```

Groups every open issue in the current repository into four
label-bucketed tables — Blocked, Other, Vanilla, Decomposed — with
mechanical sort and a copy-pasteable slash command per row. Supports
filter flags to drop or restrict sections, plus server-side narrowing
filters (`--label`, `--milestone`). Read-only — never creates, edits,
or closes issues.

---

## What It Does

1. Runs `bin/flow analyze-issues` which calls `gh issue list`
   internally, parses the JSON, detects FLOW labels (`Decomposed`,
   `Blocked`, `Vanilla`, `Flow In-Progress`, `Triage In-Progress`),
   collects per-row `assignees`, and resolves native GitHub
   `blocked_by` entries to `[{number, url}]` pairs.
2. Walks the flat `issues` array and assigns each row to the first
   matching bucket: **Blocked** (`blocked == true`), then
   **Decomposed** (`decomposed == true`), then **Vanilla**
   (`vanilla == true`), then **Other**.
3. Renders four markdown tables in order. The Blocked section has
   five columns (Issue #, Title, Assignee, Blocked By, Command);
   the other three sections have four (Issue #, Title, Assignee,
   Command). Issue # cells render as `[#N](url)` markdown links;
   `Blocked By` cells render the same way for each blocker entry.
4. Bucket-specific Command cells render copy-pasteable slash
   commands: Other → `/flow:flow-explore work on issue #N`,
   Vanilla → `/flow:flow-plan #N`, Decomposed →
   `/flow:flow-start #N`. The Blocked section's Command cell is
   suppressed. Rows with `flow_in_progress == true` (🟡, Decomposed
   section) and `triage_in_progress == true` (🔍, Other section)
   carry a colored prefix on the bold Title cell and suppress the
   Command cell — the row signals "someone else owns this".
5. Sort within sections: Blocked and Vanilla by issue number
   descending; Other and Decomposed cluster colored rows first
   (🔍 / 🟡), then sort by issue number descending within each
   cluster. Empty cells render as `—`.

---

## Filter Flags

Filtering happens at the Rust layer — `bin/flow analyze-issues`
returns a pre-filtered `issues` array and the renderer buckets
whatever it receives. Flags are mutually exclusive within each
family.

| Flag | Effect |
|------|--------|
| `--ready` | Rust drops blocked rows; no Blocked section appears in output. |
| `--blocked` | Rust keeps only blocked rows; only Blocked section appears. |
| `--decomposed` | Rust keeps only decomposed rows; only Decomposed section appears. |
| `--quick-start` | Rust keeps decomposed + non-blocked + non-Flow-In-Progress rows; no 🟡 cluster. |
| `--label <name>` | Server-side filter passed to `gh issue list` (repeatable; AND logic). |
| `--milestone <title>` | Server-side milestone filter (single value; by title or number). |

`--label` and `--milestone` compose with the section flags. No flag
renders all four sections in order.

---

## Gates

- Read-only — never creates, edits, or closes issues.
- Display-only — no AskUserQuestion prompts.
- Bucketing and sort are mechanical — no LLM judgment.
