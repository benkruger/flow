---
name: flow-code-review
description: "Phase 4: Code Review — three lenses on the same diff: clarity, correctness, and safety. Invokes /simplify, /review, and /security-review with a commit after each step."
model: opus
---

# FLOW Code Review — Phase 4: Code Review

## Usage

```text
/flow:flow-code-review
/flow:flow-code-review --auto
/flow:flow-code-review --manual
```

- `/flow:flow-code-review` — uses configured mode from the state file (default: manual)
- `/flow:flow-code-review --auto` — auto-fix and auto-commit all findings, auto-advance to Learn
- `/flow:flow-code-review --manual` — requires explicit approval of changes and routing decisions

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
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-code-review.commit` and `skills.flow-code-review.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.22.0 — Phase 4: Code Review — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-code-review --action enter
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

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The
three review steps use Claude's built-in commands which apply
language-aware checks automatically. The CLAUDE.md conventions inform
fix decisions.

---

## Step 1 — Simplify

Invoke Claude Code's built-in `/simplify` skill using the Skill tool.
`/simplify` refactors code for clarity, reduces complexity, and improves
naming while preserving exact functionality. It is safe to run here
because Phase 3 (Code) tests already verified all behavior.

Wait for `/simplify` to complete and report its changes.

Show the user what `/simplify` changed:

```bash
git diff HEAD
```

Render the diff inline in your response.

**If there are no changes** (empty diff), skip the commit and proceed
directly to Step 2.

**If there are changes and commit=auto**, skip the AskUserQuestion and
proceed directly to commit. The diff is still shown for visibility.

**If there are changes and commit=manual**, use AskUserQuestion:

> "Accept /simplify refactoring?"
>
> - **Yes, commit these changes** — accept and proceed to commit
> - **No, revert** — undo the simplifications
> - **Edit manually** — make specific changes before committing
> - **Go back to Code** — revert changes and return to Code phase

**If "Edit manually"**: The user will describe changes. After editing,
run `git diff HEAD` again to show the revised diff. Then ask again:
"Ready to commit?" with the two options: **Yes, commit** or **No, revert**.

**If "No, revert"**: Run `git restore .` to discard `/simplify`'s changes,
skip the commit, and proceed to Step 2.

**If "Go back to Code"**: Run `git restore .` to discard changes, then
follow the back-navigation instructions below.

**Commit**: Run `bin/flow ci` first. If green: if commit=auto, use
`/flow:flow-commit --auto`; otherwise use `/flow:flow-commit`.

Without pausing or asking for confirmation, continue to Step 2.

---

## Step 2 — Review

Read `pr_number` from the state file. Read `plan_file` from the state
file to get the plan file path. Use the Read tool to read the plan file.

Invoke Claude's built-in review command on the PR:

```text
/review <pr_number>
```

This analyzes the full diff for code quality, correctness, and test
coverage using Claude's language-aware analysis.

If `/review` reports no findings, show the Review summary with zero
findings listed, then without pausing continue to Step 3.

### Fix every finding

For each finding from the review:

**Minor finding** (style, missing option, small oversight):

- Fix it directly
- Describe what was fixed and why

**Significant finding** (logic error, missing risk coverage, plan mismatch):

If commit=auto, fix it directly without asking.

If commit=manual, use AskUserQuestion:

> "Found a significant issue: &lt;description&gt;. How would you like to proceed?"
>
> - **Fix it here in Code Review**
> - **Go back to Code**
> - **Go back to Plan**

After fixing findings, run:

```bash
bin/flow ci
```

<HARD-GATE>
`bin/flow ci` must be green before proceeding to Step 3.
Any fix made during Review requires `bin/flow ci` to run again.
</HARD-GATE>

If fixes were made, if commit=auto use `/flow:flow-commit --auto`,
otherwise use `/flow:flow-commit` for the Review fixes.

### Review summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Code Review — Step 2: Review — SUMMARY
============================================

  Findings fixed
  --------------
  - <description of fix and why>
  - <description of fix and why>

  bin/flow ci       : ✓ green

============================================
```
````

Without pausing or asking for confirmation, continue to Step 3.

---

## Step 3 — Security

Invoke Claude's built-in security review command:

```text
/security-review
```

This analyzes the branch diff for security vulnerabilities using Claude's
language-aware security analysis.

### Fix every finding

For each finding from the security review:

1. Fix the issue in code
2. Run `bin/flow ci`
3. If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise invoke `/flow:flow-commit`.
4. Move to the next finding

<HARD-GATE>
`bin/flow ci` must be green after every fix. Do not move to the next
finding until the current fix passes `bin/flow ci` and is committed.
</HARD-GATE>

Repeat until all findings are fixed.

If no findings, skip the commit. Show the Security summary with zero
findings, then without pausing continue to Done.

### Security summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Code Review — Step 3: Security — SUMMARY
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

Without pausing or asking for confirmation, continue to Done.

---

## Back Navigation

Use AskUserQuestion if a finding is too significant to fix in Code Review:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something

**Go back to Code:** update Phase 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:flow-code`.

**Go back to Plan:** update Phases 4 and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-code-review --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.22.0 — Phase 4: Code Review — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-learn` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 4: Code Review is complete. Ready to begin Phase 5: Learn?"
>
> - **Yes, start Phase 5 now** — invoke `flow:flow-learn`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 5 now" and "Not yet"

**If Yes** — invoke `flow:flow-learn` using the Skill tool.

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

- Always run `bin/flow ci` after any fix made during Code Review
- Never transition to Learn unless `bin/flow ci` is green
- Fix every finding from `/review` and `/security-review` — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- Each step (Simplify, Review, Security) gets its own commit when changes are made
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- After each step (Simplify, Review, Security) completes, immediately continue to the next step — never pause or wait for user input between steps
