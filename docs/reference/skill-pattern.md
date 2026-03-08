---
title: Phase Skill Pattern
nav_order: 12
parent: Reference
---

# Phase Skill Pattern

Every phase skill follows the same structure. Use this as the template
when building new phase skills.

---

## Standard Structure

```text
1. HARD-GATE entry check (tool-based — checks previous phase complete)
2. Announce banner
3. Update state file — set phase to in_progress, record session_started_at
4. cd into worktree from state file
5. [Phase-specific work]
6. Update state file — set phase to complete, calculate cumulative_seconds
7. Invoke flow:flow-status  ← always, right before the transition question
8. AskUserQuestion — "Phase X: Name is complete. Ready to begin Phase X+1?"
   - Yes, start Phase X+1 now → invoke next phase skill via Skill tool
   - Not yet → print paused banner
   - I have a correction or learning to capture → invoke flow:flow-note, then re-ask
```

---

## Announce Banner

````text
```
============================================
  FLOW — Phase N: Name — STARTING
============================================
```
````

## Paused Banner

````text
```
============================================
  FLOW — Paused
  Run /flow-continue when ready to continue.
============================================
```
````

## Completion Banner (shown after Yes is selected)

````text
```
============================================
  FLOW — Phase N: Name — COMPLETE
============================================
```
````

---

## State File Updates

**On phase entry:**

```bash
bin/flow phase-transition --phase <name> --action enter
```

**On phase exit:**

```bash
bin/flow phase-transition --phase <name> --action complete
```

The `phase-transition` script handles all timing, counters, and status
fields. Skills must never compute timestamps, time differences, or
counter increments — all computation goes through `bin/flow` commands.

For mid-phase timestamp fields (`scanned_at`, plan file path), use:

```bash
bin/flow set-timestamp --set <path>=NOW
```

---

## HARD-GATE Template

Replace `PREV` with the previous phase number and `PREV_NAME` with its name:

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow-start first."
4. Check `phases.PREV.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase PREV: PREV_NAME must be
     complete first."

---

## Sub-Agent Pattern

Phases with sub-agents: Review, Security.
Start uses a Sonnet sub-agent for CI failures.
Plan uses Claude Code's native plan mode (`EnterPlanMode`/`ExitPlanMode`).
Phases without sub-agents: Code, Learning, Cleanup.

The pattern for Review and Security:

```text
1. Main conversation determines WHAT to check (from plan file + state file)
2. Launch sub-agent via Task tool with subagent_type: "general-purpose"
3. Sub-agent reads files, returns structured findings
4. Main conversation uses findings to do the phase work
5. Main conversation persists relevant findings to state file
```

Sub-agents do NOT: make decisions, write code, modify state, interact with users.
They read and report. The main conversation decides.

**Code phase rationale:** By the time Code starts, the plan file contains
thorough exploration, a validated approach, identified risks, and ordered
tasks — all produced during Plan phase. Code trusts the plan. It reads
the plan file and the specific file it's modifying — nothing more.

---

## Note Capture at Transitions

Every phase transition (Phases 1-6) includes a third option:

```text
"Phase X: Name is complete. Ready to begin Phase X+1?"
- Yes, start Phase X+1 now
- Not yet
- I have a correction or learning to capture
```

If the user picks option 3:
1. Ask what they want to capture (open text)
2. Invoke `/flow-note` with their message
3. Re-ask the transition question with only "Yes" and "Not yet"

This is separate from the automatic correction capture in the session hook.
The hook catches corrections as they happen mid-conversation. The transition
prompt catches things the user thought of but didn't say.

---

## Rules Every Phase Skill Follows

- Never skip the HARD-GATE
- Always cd into the worktree before running any commands
- Always invoke `flow:flow-status` before the transition question
- Always use AskUserQuestion for the transition — never print "type /flow-next"
- Yes → invoke next skill via Skill tool
- Not yet → paused banner only
- **Always run `bin/flow ci` before any state transition that touches code**
