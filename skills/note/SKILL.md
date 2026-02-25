---
name: note
description: "Invoke automatically whenever the user corrects Claude, disagrees with a response, or says something was wrong. Also invoke explicitly with /flow:note to capture any learning mid-session. Fast — captures and continues without interrupting flow."
---

# FLOW Note

Capture a correction or learning to the state file immediately.
This skill must be fast — capture and continue, no interruption.

## When to invoke automatically

Invoke this skill BEFORE replying whenever the user:

- Corrects a mistake Claude made
- Says Claude was wrong about something
- Disagrees with a Claude response
- Clarifies something Claude misunderstood
- Says "no", "that's not right", "actually", "you missed", "I disagree"

Do not wait to be asked. Capture first, then respond.

## Steps

### Step 1 — Find the state file

1. Get the current branch: run `git branch --show-current`.
2. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: skip silently — do not interrupt the session.

If no state file is found, skip silently — do not interrupt the session.

### Step 2 — Write the note

Compose the note text as a reusable pattern, not a specific complaint:

- Bad: *"User said I was wrong about branches"*
- Good: *"Never assume branch-behind is unlikely in a multi-session workflow — multiple active sessions means branches regularly fall behind main"*

- Bad: *"I suggested rebase, user rejected"*
- Good: *"Always merge, never rebase — rebasing is forbidden in this workflow"*

The note should read as something useful to a future session, not a log of what happened.

Run the append-note script with the state file path, current phase, type, and note text:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow append-note <state_file_path> --phase <N> --type correction --note "<note text>"
```

The script appends a structured note with phase, phase\_name, timestamp, type, and note fields to the state file's `notes` array.

### Step 3 — Confirm quietly

Print one line only:

```text
[note captured]
```

Then continue with the response immediately.

## For explicit invocation

When the user types `/flow:note <message>`:
- Use their message as the note text directly
- Still write to `state["notes"]` with current phase and timestamp
- Print `[note captured]` and stop

## Rules

- Never interrupt the conversation — capture and continue
- Always write as a reusable pattern
- If no state file exists, skip silently — never block a session
- Notes survive compaction and session restarts
