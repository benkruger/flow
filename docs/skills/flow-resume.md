---
title: /flow:resume
nav_order: 4
parent: Skills
---

# /flow:resume

**Phase:** Any

**Usage:** `/flow:resume`

Resumes the current FLOW feature. Behaves differently depending on context:

- **Mid-session** (you already have context) — re-asks the last phase transition question
- **New session** (no context) — reads the state file, shows status, then asks

---

## What It Does

### Mid-session

If you are in an active session and already know the current phase:

1. Re-asks the phase transition question that was most recently declined
2. If the user says "Yes" — invokes the current phase skill
3. If the user says "Not yet" — prints the paused banner

### New session

If this is a new session or context has been compacted:

1. Finds `.flow-states/<branch>.json` from the project root
2. Changes into the worktree
3. Invokes `/flow:status` to display current state
4. Asks whether to continue the current phase

---

## Gates

- Read-only until the user confirms — never modifies state unprompted
- If no state file is found, reports it and stops
