---
name: flow-note
description: "Capture a correction or learning to the FLOW state file. Invoke explicitly with /flow:flow-note or when directed by session context. Fast — captures and continues without interrupting flow."
---

# FLOW Note

Capture a correction or learning to the state file immediately.
This skill must be fast — capture and continue, no interruption.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name.

## When to invoke automatically

Invoke this skill BEFORE replying whenever the user:

- Corrects a mistake Claude made
- Says Claude was wrong about something
- Disagrees with a Claude response
- Clarifies something Claude misunderstood
- Says "no", "that's not right", "actually", "you missed", "I disagree"

Do not wait to be asked. Capture first, then respond.

## Steps

### Step 1 — Write the note

Compose the note text as a reusable pattern, not a specific complaint:

- Bad: *"User said I was wrong about branches"*
- Good: *"Never assume branch-behind is unlikely in a multi-session workflow — multiple active sessions means branches regularly fall behind main"*

- Bad: *"I suggested rebase, user rejected"*
- Good: *"Always merge, never rebase — rebasing is forbidden in this workflow"*

The note should read as something useful to a future session, not a log of what happened.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow append-note --note "<note_text>"
```

The script derives the state file path and current phase internally.

The script outputs JSON:

- `{"status": "no_state"}` — no state file exists. Skip silently — do not
  interrupt the session. Continue with your response.
- `{"status": "ok", "note_count": N}` — note captured. Proceed to Step 2.
- `{"status": "error", "message": "..."}` — show the error message and stop.

### Step 2 — Confirm quietly

Print one line only:

```text
[note captured]
```

Then continue with the response immediately.

## For explicit invocation

When the user types `/flow:flow-note <message>`:
- Use their message as the note text directly
- Still write to `state["notes"]` with current phase and timestamp
- Print `[note captured]` and stop

## Rules

- Never interrupt the conversation — capture and continue
- Always write as a reusable pattern
- If no state file exists, skip silently — never block a session
- Notes survive compaction and session restarts
