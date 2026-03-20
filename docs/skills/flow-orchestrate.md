---
title: /flow-orchestrate
nav_order: 17
parent: Skills
---

# /flow-orchestrate

**Phase:** Any

**Usage:** `/flow-orchestrate`

Processes decomposed issues sequentially overnight via `flow-start --auto`. Fetches open issues labeled "Decomposed", filters out in-progress issues, and runs the full Start-Plan-Code-Review-Learn-Complete lifecycle for each one. Generates a morning report with results.

---

## What It Does

1. Fetches open issues with the "Decomposed" label, excludes those with "Flow In-Progress"
2. Creates an orchestration state file at `.flow-states/orchestrate.json` to track the queue
3. For each issue in the queue (sorted by issue number ascending):
   - Invokes `flow-start --auto` with the issue title and number
   - The full 6-phase lifecycle runs autonomously
   - Detects the outcome from GitHub PR state (merged = completed, closed = failed)
   - Cleans up stuck features via `flow-abort --auto` if needed
4. Generates a summary report at `.flow-states/orchestrate-summary.md`
5. Marks the orchestration complete

---

## Morning Report

The report is delivered in two ways:

- **End of session:** Rendered inline after the last issue completes
- **Next session start:** The session-start hook detects the completed orchestration, presents the report, and cleans up the state files

---

## Compaction Survival

The orchestrator state file (`.flow-states/orchestrate.json`) tracks the queue position and per-issue outcomes. Self-invocation after each feature keeps the working context bounded. The session-start hook detects in-progress orchestrations and injects resume instructions after compaction.

---

## Multi-Run Lifecycle

The "Decomposed" label is the queue:

- **Completed issues** are closed by `flow-complete` and excluded from the next run
- **Failed issues** retain the label and re-enter the queue on subsequent runs
- **New issues** decomposed during the day enter automatically

No configuration or manual queue management needed.

---

## Gates

- One orchestration per machine at a time (state file acts as lock)
- No parallel issue processing — sequential only
- No retries for failed issues in V1
