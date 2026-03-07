---
name: flow-simplify
description: "Phase 4: Simplify — invoke /simplify to refactor code for clarity, then auto-commit. Safe because tests already passed in Code phase."
model: sonnet
---

# FLOW Simplify — Phase 4: Simplify

## Usage

```text
/flow:flow-simplify
/flow:flow-simplify --auto
/flow:flow-simplify --manual
```

- `/flow:flow-simplify` — uses configured mode from `.flow.json` (default: manual)
- `/flow:flow-simplify --auto` — accept refactoring without approval (still show diff), auto-advance to Review
- `/flow:flow-simplify --manual` — requires explicit approval of refactoring changes

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
3. Check `phases.flow-code.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 3: Code must be
     complete. Run /flow:flow-code first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.flow-simplify.commit` and `skills.flow-simplify.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.0 — Phase 4: Simplify — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-simplify --action enter
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
YYYY-MM-DDTHH:MM:SSZ [Phase 4] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

---

## Purpose

This phase runs Claude Code's built-in `/simplify` skill on the PR diff.
`/simplify` refactors code for clarity, reduces complexity, and improves
naming while preserving exact functionality. It is safe to run here because
Phase 3 (Code) tests already verified all behavior is preserved.

---

## Step 1 — Invoke /simplify

Invoke the `/simplify` skill using the Skill tool:

```text
The /simplify skill will refactor the current feature's code for clarity.
It removes unnecessary abstractions, simplifies conditionals, improves naming,
and enforces CLAUDE.md rules. It preserves exact functionality — all behavior
stays the same.
```

Wait for `/simplify` to complete and report its changes.

---

## Step 2 — Review the diff

Show the user what `/simplify` changed:

```bash
git diff HEAD
```

Render the diff inline in your response.

**If commit=auto**, skip the AskUserQuestion and proceed directly to Step 3
(auto-commit). The diff is still shown for visibility.

**If commit=manual**, use AskUserQuestion:

> "Accept /simplify refactoring?"
>
> - **Yes, commit these changes** — accept and proceed to auto-commit
> - **No, revert** — undo the simplifications
> - **Edit manually** — make specific changes before committing
> - **Go back to Code** — revert changes and return to Code phase

**If "Edit manually"**: The user will describe changes. After editing,
run `git diff HEAD` again to show the revised diff. Then ask again:
"Ready to commit?" with the two options: **Yes, commit** or **No, revert**.

**If "No, revert"**: Run `git restore .` to discard `/simplify`'s changes,
then proceed directly to Done.

**If "Yes, commit"**: Proceed to Step 3.

**If "Go back to Code"**: Run `git restore .` to discard changes,
update Phase 4 to `pending`, Phase 3 to `in_progress`, then invoke
`flow:flow-code`.

---

## Step 3 — Auto-commit

Automatically commit the `/simplify` changes without additional approval
(the user already approved in Step 2):

If commit=auto, use `/flow:flow-commit --auto`. Otherwise, use `/flow:flow-commit`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-simplify --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.0 — Phase 4: Simplify — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-review` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 4: Simplify is complete. Ready to begin Phase 5: Review?"
>
> - **Yes, start Phase 5 now** — invoke `flow:flow-review`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 5 now" and "Not yet"

**If Yes** — invoke `flow:flow-review` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Do not narrate or skip — run /simplify immediately, show the diff, get approval
- `/simplify` is invoked, not optional — if it can't run, report the error and stop
- Do not commit changes without showing the diff from Step 2
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
