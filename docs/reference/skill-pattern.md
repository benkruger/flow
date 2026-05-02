---
title: Phase Skill Pattern
nav_order: 12
parent: Reference
---

# Phase Skill Pattern

Every phase skill follows the same structure. Use this as the template
when building new phase skills.

---

## Naming Convention

All skill directories must use the `flow-` prefix: `skills/flow-<name>/`.
This ensures consistent invocation as `/flow:flow-<name>` across the
entire plugin namespace. `test_structural.py` enforces this convention —
CI will fail if a skill directory does not start with `flow-`.

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
──────────────────────────────────────────────────
  FLOW — Phase N: Name — STARTING
──────────────────────────────────────────────────
```
````

## Paused Banner

````text
```
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run the phase command when ready.
══════════════════════════════════════════════════
```
````

## Completion Banner (shown after Yes is selected)

````text
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW — Phase N: Name — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

---

## State File Updates

**On phase entry (Code, Code Review, Learn):**

```bash
bin/flow phase-enter --phase <name> --steps-total <n>
```

**On phase entry (Plan, Complete — legacy pattern):**

```bash
bin/flow phase-transition --phase <name> --action enter
```

**On phase exit (Start, Code, Code Review, Learn):**

```bash
bin/flow phase-finalize --phase <name> --branch <branch> --thread-ts <ts>
```

**On phase exit (Plan, Complete — legacy pattern):**

```bash
bin/flow phase-transition --phase <name> --action complete
```

These commands handle all timing, counters, and status fields. Skills
must never compute timestamps, time differences, or counter increments
— all computation goes through `bin/flow` commands.

For mid-phase timestamp fields (`scanned_at`, plan file path), use:

```bash
bin/flow set-timestamp --set <path>=NOW
```

---

## HARD-GATE Template

Replace `PREV` with the previous phase number and `PREV_NAME` with its name:

1. Run `git worktree list --porcelain`. Note the path on the first
   `worktree` line (this is the project root). Find the `worktree` entry
   whose path matches your current working directory — the
   `branch refs/heads/<name>` line in that entry is the current branch
   (strip the `refs/heads/` prefix).
2. Use the Read tool to read `<project_root>/.flow-states/<branch>/state.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow-start first."
3. Check `phases.PREV.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase PREV: PREV_NAME must be
     complete first."

---

## Sub-Agent Pattern

FLOW uses six custom plugin sub-agents in `agents/*.md`: ci-fixer, reviewer,
pre-mortem, adversarial, learn-analyst, and documentation. The `PreToolUse`
hook (`bin/flow hook validate-pretool`) is registered globally in `hooks/hooks.json`,
enforcing tool restrictions on all Bash calls — including those from
built-in skills' sub-agents. The hook validates three layers: compound
command blocking, file-read command blocking, and whitelist enforcement
against `.claude/settings.json` allow patterns. Commands not matching any
`Bash(...)` pattern are blocked with exit 2. Agent frontmatter must only
use supported keys — unsupported keys like `hooks` can cause loading failures.

Start and Complete use ci-fixer for CI failure diagnosis and fix.
Plan invokes the `decompose` plugin for DAG-based task decomposition.
Code Review launches four agents in parallel — reviewer, pre-mortem,
adversarial, and documentation — for cognitively isolated analysis.
The parent session gathers context, triages findings, and fixes.
Code has no sub-agents.
Learn uses learn-analyst (cognitively isolated compliance audit).

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
- **If continue=auto** → invoke next skill directly via Skill tool as the final action — no `flow:flow-status`, no AskUserQuestion
- **If continue=manual** → invoke `flow:flow-status`, then use AskUserQuestion for the transition
- Yes → invoke next skill via Skill tool
- Not yet → paused banner only
- **Always run `bin/flow ci` before any state transition that touches code**
