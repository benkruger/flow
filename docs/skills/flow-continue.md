---
title: /flow-continue
nav_order: 4
parent: Skills
---

# /flow-continue

**Phase:** Any

**Usage:** `/flow-continue`

Resumes the current FLOW feature. The session hook provides awareness of in-progress features, but this skill is the explicit action that resumes work. Behaves differently depending on context:

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

1. Runs `bin/flow continue-context` to load branch, state, and status panel in one call
2. Changes into the worktree and displays the status panel
3. Asks whether to continue the current phase

---

## Gates

- Read-only until the user confirms — never modifies state unprompted
- If no state file is found, reports it and stops
