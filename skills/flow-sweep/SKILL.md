---
name: flow-sweep
description: "Batch-process GitHub issues in parallel — spawn autonomous workers, track progress, show dashboard."
---

# FLOW Sweep

Fetch open issues, spawn autonomous worker agents to fix them in
parallel, and display a status dashboard. Each worker gets its own
worktree, implements a fix, runs tests, and opens a PR.

## Usage

```text
/flow:flow-sweep
/flow:flow-sweep --status
/flow:flow-sweep --cleanup
```

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.29.0 — flow:flow-sweep — STARTING
============================================
```
````

## Mode Detection

Check the arguments:

- **`--status`** — skip to Step 5 (Dashboard)
- **`--cleanup`** — skip to Step 6 (Cleanup)
- **No flags** — proceed to Step 1

## Step 1 — Fetch Issues

Run:

```bash
gh issue list --state open --json number,title,labels,createdAt,body --limit 100
```

Parse the JSON output. If there are no open issues, print the
COMPLETE banner and stop.

## Step 2 — Present and Select

Display the issues in a numbered list with title, age, and labels.
Ask the user which issues to process:

- **"all"** — process all issues (subject to concurrency limit)
- **Specific numbers** — e.g., "42 43 51"
- **"cancel"** — abort

Also ask the user for a concurrency limit (default: 3). This is
how many workers run simultaneously.

## Step 3 — Initialize Sweep State

Build a JSON array of the selected issues with `number` and `title`
fields, then create the sweep state file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-init --issues '<JSON_ARRAY>' --limit <N>
```

For example, if the user selected issues 42 and 43:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-init --issues '[{"number":42,"title":"Fix login"},{"number":43,"title":"Add feature"}]' --limit 3
```

This creates `.flow-states/sweep.json` with all issues set to
`queued` and timestamps generated server-side.

## Step 4 — Spawn Workers

For each issue up to the concurrency limit, spawn a worker agent:

- Use the Agent tool with `subagent_type: "flow:issue-worker"`
- Set `run_in_background: true`
- Set `name: "worker-<NUMBER>"`
- Set `isolation: "worktree"` so each agent gets its own branch
- Include in the prompt: the issue number, title, full body text,
  and any relevant labels

Before spawning each worker, update its status:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-update --issue <NUMBER> --status in_progress
```

Issues beyond the concurrency limit stay queued. When a background
agent completes, check if there are queued issues and spawn the
next worker.

After spawning all initial workers, display the dashboard (Step 5).

Tell the user they can ask about status anytime, or run
`/flow:flow-sweep --status` for the dashboard.

## Step 5 — Dashboard

Run:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-status
```

Display the output. If a specific issue is asked about and the
worker agent is still running (you know its name), use SendMessage
to query it for a live status update.

## Step 6 — Cleanup

For each issue in sweep.json with status `complete` or `failed`:

1. If the worktree path exists, remove it:

   ```bash
   git worktree remove <worktree_path>
   ```

2. Clear the issue from sweep.json by updating status to show it
   was cleaned.

After cleanup, display the dashboard.

## Handling Worker Completion

When a background agent completes and you are notified:

1. Parse the agent's result for: status, PR URL, PR number, files
   changed, errors.

2. Update the sweep state:

   ```bash
   exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-update --issue <NUMBER> --status complete --pr-url "<URL>" --pr-number <N>
   ```

   Or if the agent failed:

   ```bash
   exec ${CLAUDE_PLUGIN_ROOT}/bin/flow sweep-update --issue <NUMBER> --status failed --error "<message>"
   ```

3. Check for queued issues. If any remain and current in-progress
   count is below the concurrency limit, spawn the next worker
   (same as Step 4).

4. Show the updated dashboard.

## Complete

After all workers have finished (or on `--status` when all are done),
output the following banner:

````markdown
```text
============================================
  FLOW v0.29.0 — flow:flow-sweep — COMPLETE
============================================
```
````

## Hard Rules

- Never modify issues directly — workers create PRs, users review
- Never force-push or rebase worker branches
- Always use `bin/flow sweep-update` for state mutations
- Never use Bash to print banners — output them as text
- Always respect the concurrency limit
- If a worker fails, log the error and continue with remaining issues
