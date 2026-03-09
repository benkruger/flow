---
name: flow-status
description: "Show current SDLC phase, PR link, timing, and what comes next. Reads .flow-states/<branch>.json. Use any time you want to know where you are in the workflow."
---

# FLOW Status

Show where you are in the FLOW workflow. Reads the state file and
prints a status panel. Read-only — never modifies anything.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.21.1 — flow:flow-status — STARTING
============================================
```
````

## Steps

### Step 1 — Run the status formatter

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow format-status
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
