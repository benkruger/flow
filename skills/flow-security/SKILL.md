---
name: flow-security
description: "Phase 6: Security — scan for security issues in the feature diff. In-flow: diff-only after Review. Standalone: full repo, report-only, no state file required."
model: opus
---

# FLOW Security — Phase 6: Security

## Usage

```text
/flow:flow-security
/flow:flow-security --auto
/flow:flow-security --manual
```

- `/flow:flow-security` — uses configured mode from `.flow.json` (default: auto)
- `/flow:flow-security --auto` — auto-advance to Learning on completion
- `/flow:flow-security --manual` — prompt before advancing to Learning

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
3. Check `phases.flow-review.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 5: Review must be
     complete. Run /flow:flow-review first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.flow-security.commit` and `skills.flow-security.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=auto, continue=auto

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 6: Security — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-security --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 6] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The security
scan uses Claude's built-in `/security-review` command which applies
language-aware security checks automatically.

---

## Step 1 — Run security review

Invoke Claude's built-in security review command:

```text
/security-review
```

This analyzes the branch diff for security vulnerabilities using Claude's
language-aware security analysis.

---

## Step 2 — Fix every finding

For each finding from the security review:

1. Fix the issue in code
2. Run `bin/flow ci`
3. If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise invoke `/flow:flow-commit`.
4. Move to the next finding

<HARD-GATE>
`bin/flow ci` must be green after every fix. Do not move to the next finding
until the current fix passes `bin/flow ci` and is committed.
</HARD-GATE>

Repeat until all findings are fixed.

If no findings, skip to Step 3.

---

## Step 3 — Present security summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 6: Security — SUMMARY
============================================

  Findings         : N
  Fixed            : N

  Findings
  --------
  - [FIXED] <description of finding>
  - [FIXED] <description of finding>

  bin/flow ci      : ✓ green

============================================
```
````

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-security --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 6: Security — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-learning` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 6: Security is complete. Ready to begin Phase 7: Learning?"
>
> - **Yes, start Phase 7 now** — invoke `flow:flow-learning`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 7 now" and "Not yet"

**If Yes** — invoke `flow:flow-learning` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Always run `bin/flow ci` after any fix made during Security
- Never transition to Learning unless `bin/flow ci` is green
- Read the full diff before starting — no partial reviews
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
