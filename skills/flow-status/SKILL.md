---
name: flow-status
description: "Show current SDLC phase, PR link, timing, and what comes next. Reads .flow-states/<branch>/state.json. Use any time you want to know where you are in the workflow."
---

# FLOW Status

Show where you are in the FLOW workflow. Reads the state file and
prints a status panel. Read-only — never modifies anything.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-status — STARTING
──────────────────────────────────────────────────
```
````

## Panel Fields

The status panel includes:

- **Feature** — derived from the branch name
- **Branch** — the git branch the flow is running on
- **Subdir** — the `relative_cwd` from the state file, shown only when
  non-empty. For a mono-repo flow started inside `api/`, this reads
  `api`. Root-level flows (empty `relative_cwd`) omit the line.
- **PR** — the PR URL if one exists
- **Elapsed** — total time since the flow started
- **Notes** — count of session notes (omitted when zero)
- Phase list with completion state and cumulative time
- Time in current phase + visit count for the active phase
- Continue/Next command to run

## Steps

### Step 1 — Run the status formatter

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow format-status
```

Check the exit code:

- **Exit 0** — stdout contains the panel text (single feature or multiple
  features). Print it inside a fenced code block (triple backticks with
  `text` language tag) so it renders as plain monospace text.

- **Exit 1** — no state file exists. Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
No FLOW feature in progress on this branch.
Start one with /flow:flow-start <feature name>.
```
````

Then stop.

- **Exit 2** — error. stderr contains the error message. Show it and stop.

## Rules

- Read-only — never modifies the state file or any other files
- Never calls TaskCreate or TaskUpdate
