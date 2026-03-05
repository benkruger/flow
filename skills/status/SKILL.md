---
name: status
description: "Show current SDLC phase, PR link, timing, and what comes next. Reads .flow-states/<branch>.json. Use any time you want to know where you are in the workflow."
---

# FLOW Status

Show where you are in the FLOW workflow. Reads the state file and
prints a status panel. Read-only — never modifies anything.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.15.0 — flow:status — STARTING
============================================
```
````

## Steps

### Step 1 — Run the status formatter

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow format-status
```

The script outputs JSON:

- `{"status": "no_state"}` — no state file exists. Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
No FLOW feature in progress on this branch.
Start one with /flow:start <feature name>.
```
````

Then stop.

- `{"status": "multiple_features", "panel": "..."}` — multiple active
  features found, none on the current branch. Print the `panel` value
  inside a fenced code block (triple backticks with `text` language
  tag). The panel lists all active features with their branch and phase.

- `{"status": "ok", "panel": "..."}` — print the `panel` value inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading.

- `{"status": "error", "message": "..."}` — show the error message and stop.

## Rules

- Read-only — never modifies the state file or any other files
- Never calls TaskCreate or TaskUpdate
