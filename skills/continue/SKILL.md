---
name: continue
description: "Resume the current FLOW feature. Mid-session: re-asks the last phase transition question. New session: reads state file, shows status, then asks."
---

# FLOW Continue

This skill behaves differently depending on whether you are mid-session
or starting fresh. Choose the right path below.

---

## Path A — Mid-session (you already have context)

If you are in an active session and already know the current phase and
feature — simply re-ask the phase transition question that was most
recently declined:

Use AskUserQuestion:

> "Ready to continue Phase X: Name?"
>
> - **Yes, continue** — invoke the phase skill using the Skill tool
> - **Not yet** — print the paused banner and stop

The Skill to invoke maps directly to the current phase:

| Current phase | Skill to invoke |
|--------------|----------------|
| 1 — Start | `flow:start` |
| 2 — Plan | `flow:plan` |
| 3 — Code | `flow:code` |
| 4 — Review | `flow:review` |
| 5 — Security | `flow:security` |
| 6 — Reflect | `flow:reflect` |
| 7 — Cleanup | `flow:cleanup` |

---

## Path B — New session (no current context)

If this is a new session or you have no context about the current
feature, rebuild from the state file:

### Step 1 — Load context

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow continue-context
```

Parse the JSON output:

- `"status": "no_state"` — report "No FLOW feature in progress on
  branch '<branch>'." and stop.
- `"status": "error"` — report the error message and stop.
- `"status": "multiple_features"` — multiple active features, none on
  the current branch. Use AskUserQuestion to present the `features`
  list (each has `feature`, `branch`, `phase_name`, `worktree`).
  Once selected, cd into that feature's `worktree` and re-run
  `bin/flow continue-context`.
- `"status": "ok"` — continue to Step 2. The response contains
  `panel`, `branch`, `worktree`, `current_phase`, `phase_name`, and
  `phase_command`.

### Step 2 — cd and show status

cd into the `worktree` path from Step 1, then print the `panel`
inside a fenced code block (triple backticks with `text` language tag).

### Step 3 — Ask the transition question

Use AskUserQuestion with the `phase_name` and `current_phase` from
Step 1:

> "Ready to continue Phase X: Name?"
>
> - **Yes, continue** — invoke the `phase_command` skill using the Skill tool
> - **Not yet** — print the paused banner and stop

---

## Paused Banner

When the user selects "Not yet", always print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````
