---
name: flow-orchestrate
description: "Process decomposed issues sequentially overnight via flow-start --auto, tracking outcomes and generating a morning report."
---

# FLOW Orchestrate

Fetch all open issues labeled "Decomposed", filter out issues already in progress, and process each sequentially via `flow-start --auto`. Each invocation runs the full Start-Plan-Code-Review-Learn-Complete lifecycle. After all issues are processed, generate a summary report.

## Usage

```text
/flow:flow-orchestrate
/flow:flow-orchestrate --continue-step
```

- `/flow:flow-orchestrate` — start a new orchestration run
- `/flow:flow-orchestrate --continue-step` — resume after processing an issue (self-invocation)

## Concurrency

Only one orchestration runs per machine at a time. The state file
(`.flow-states/orchestrate.json`) acts as a lock — if it exists without
`completed_at`, another orchestration is in progress.

Individual features within the orchestration use the standard FLOW
concurrency model: branch-scoped worktrees and state files, GitHub for
shared state coordination.

## Self-Invocation Check

If `--continue-step` was passed, skip the Announce banner and Steps 1-2.
Proceed directly to the Resume Check section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-orchestrate — STARTING
──────────────────────────────────────────────────
```
````

---

## Step 1 — Fetch decomposed issues

```bash
gh issue list --state open --label Decomposed --json number,title,labels,body,url --limit 100
```

Parse the JSON output. Filter out any issues that have the "Flow In-Progress" label — these are already being worked by another FLOW feature.

If no issues remain after filtering, output:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-orchestrate — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  No decomposed issues to process.
```
````

Stop.

## Step 2 — Initialize orchestration state

Build the queue from the filtered issues. Sort by issue number ascending. Write the queue to a temporary file and create the orchestration state:

Write the queue JSON to `.flow-states/orchestrate-queue.json` using the Write tool. Each item must have `issue_number` (integer) and `title` (string) fields.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-state --create --queue-file .flow-states/orchestrate-queue.json --state-dir .flow-states
```

If the response indicates an orchestration is already in progress, stop and report.

Log the queue:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log orchestrate "[Orchestrate] Started — N issues queued"
```

Display the queue as a rich markdown table matching the `flow-issues` format.
Use the `body` and `labels` fields from Step 1 to categorize, assess impact,
and prioritize each issue using the same logic as `flow-issues`:

- **Categorize** by label (Rule, Flow, Flaky Test, Tech Debt, Documentation
  Drift) with content-based fallback (Bug, Enhancement, Other)
- **Impact** from cross-area scope, force-multiplier keywords, acceptance
  criteria density, and dependency count (High/Medium/Low)
- **Priority** from category defaults with impact and blocking modifiers
- **Sort** by priority (High first), then decomposed before non-decomposed,
  then oldest first

Output the table inline with columns: `Order`, `Priority`, `Impact`,
`Labels`, `#`, `Title`, `Rationale`. The `#` column uses a markdown link
`[#N](issue_url)`.

---

## Resume Check

Read the orchestration state to find the next pending issue:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-state --next --state-file .flow-states/orchestrate.json
```

- If `"status": "done"` — all issues processed. Skip to Done.
- If `"status": "ok"` — process this issue. Continue to Step 3.
- If `"status": "error"` — report and stop.

---

## Step 3 — Process next issue

The `--next` response includes `index`, `issue_number`, and `title`.

**Mark issue as started:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-state --start-issue <index> --state-file .flow-states/orchestrate.json
```

Log it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log orchestrate "[Orchestrate] Starting #<issue_number> — <title>"
```

**Invoke flow-start:**

Invoke `flow:flow-start --auto <title> #<issue_number>` using the Skill tool.

This runs the full lifecycle: Start, Plan, Code, Code Review, Learn, Complete.

**Detect outcome:**

After `flow-start --auto` returns, determine the outcome by checking GitHub state.

Check the PR state for the branch. Use `gh pr view` with the branch name:

```bash
gh pr view <branch> --json number,state,mergedAt,url
```

Where `<branch>` is derived from the issue title (the same branch name `flow-start` would create).

Determine outcome:

- Command succeeds and `mergedAt` is not null → **completed**
- Command succeeds and state is "CLOSED" → **failed**
- Command fails (no PR for that branch) → check if a `.flow-states/<branch>.json` exists using the Glob tool. If it does, the feature is stuck — record as **failed** with reason "Feature did not complete"

**Record outcome:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-state --record-outcome <index> --outcome <completed|failed> --pr-url <pr_url> --branch <branch> --state-file .flow-states/orchestrate.json
```

For failed outcomes, add `--reason "<reason>"`.

**Clean up stuck features:**

If the outcome is **failed** and a state file still exists for the branch, invoke `flow:flow-abort --auto` to clean up the worktree, close the PR, and remove the state file.

**Log and continue:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log orchestrate "[Orchestrate] #<issue_number> — <outcome>"
```

Self-invoke to process the next issue. Invoke `flow:flow-orchestrate --continue-step` using the Skill tool as your final action. Do not output anything else after this invocation.

---

## Done

All issues have been processed.

### Mark complete

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-state --complete --state-file .flow-states/orchestrate.json
```

### Generate report

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow orchestrate-report --state-file .flow-states/orchestrate.json --output-dir .flow-states
```

### Present report

Read `.flow-states/orchestrate-summary.md` using the Read tool and render the full content inline in your response.

Output the completion banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-orchestrate — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

The session-start hook will present the report again on the next session start if the terminal was closed before the user saw it.

---

## Hard Rules

- Never process issues in parallel — one at a time, sequentially
- Never retry failed issues — log them and move on
- Never modify the decomposed label on issues — completed issues are closed by flow-complete, failed issues retain the label for the next run
- Never skip flow-start — the orchestrator delegates all phase execution
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead
