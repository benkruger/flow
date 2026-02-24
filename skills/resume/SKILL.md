---
name: resume
description: "Resume the current FLOW feature. Mid-session: re-asks the last phase transition question. New session: reads state file, shows status, then asks."
---

# FLOW Resume

This skill behaves differently depending on whether you are mid-session
or starting fresh. Choose the right path below.

---

## Path A — Mid-session (you already have context)

If you are in an active session and already know the current phase and
feature — simply re-ask the phase transition question that was most
recently declined:

Use AskUserQuestion:

> "Ready to continue Phase X: Name?"
> - **Yes, continue** — invoke the phase skill using the Skill tool
> - **Not yet** — print the paused banner and stop

The Skill to invoke maps directly to the current phase:

| Current phase | Skill to invoke |
|--------------|----------------|
| 1 — Start | `flow:start` |
| 2 — Research | `flow:research` |
| 3 — Design | `flow:design` |
| 4 — Plan | `flow:plan` |
| 5 — Code | `flow:code` |
| 6 — Review | `flow:review` |
| 7 — Reflect | `flow:reflect` |
| 8 — Cleanup | `flow:cleanup` |

---

## Path B — New session (no current context)

If this is a new session or you have no context about the current
feature, rebuild from the state file:

### Step 1 — Find the state file

1. Get the current branch: run `git branch --show-current`.
2. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
3. Use the Read tool to read `<project_root>/.claude/flow-states/<branch>.json`.
   - If the file does not exist: report "No FLOW feature in progress on
     branch '<branch>'." and stop.

If no state file is found — report it and stop.

### Step 2 — cd into the worktree

Read `worktree` from the state file and cd there.

### Step 3 — Show status panel

Invoke the `flow:status` skill to display current state.

### Step 4 — Ask the transition question

Use AskUserQuestion:

> "Ready to continue Phase X: Name?"
> - **Yes, continue** — invoke the phase skill using the Skill tool
> - **Not yet** — print the paused banner and stop

---

## Paused Banner

When the user selects "Not yet", always print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```
````