---
name: flow-start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades dependencies, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:flow-start <feature name words>"
model: haiku
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:flow-start invoice pdf export
/flow:flow-start --auto invoice pdf export
/flow:flow-start --manual invoice pdf export
```

Arguments become the feature name (flags are not included in the name). Words are joined with hyphens:

- Branch: `invoice-pdf-export`
- Worktree: `.worktrees/invoice-pdf-export`
- PR title: `Invoice Pdf Export`

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen.

<HARD-GATE>
Do NOT proceed if the feature name is missing. Ask the user:
"What is the feature name? e.g. /flow:flow-start invoice pdf export"
</HARD-GATE>

## Mode Resolution

1. If `--auto` was passed → continue=auto
2. If `--manual` was passed → continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.flow-start.continue`.
4. If `.flow.json` has no `skills` key → use built-in default: continue=manual

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 1: Start — STARTING
============================================
```
````

## Logging

After every Bash command in Steps 5–8, log it to `.flow-states/<branch>.log`. Step 4 handles its own logging internally.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 1] Step X — desc (exit EC)
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

### Step 1 — Version gate

Run the version check before anything else:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow init-check
```

Parse the JSON output. If `"status": "error"`, tell the user to run `/flow:flow-init` and stop. Do not proceed to any further steps.

<HARD-GATE>
Do NOT proceed if version check fails. Tell the user to run `/flow:flow-init` and stop.
</HARD-GATE>

After init-check passes, check for a newer release:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow upgrade-check
```

Parse the JSON output:
- `"status": "current"` — proceed silently
- `"status": "unknown"` — proceed silently (best-effort check)
- `"status": "upgrade_available"` — show this notice, then continue:

````markdown
```text
╔══════════════════════════════════════════════╗
║  FLOW update available: v{installed} → v{latest}
║
║  To upgrade:
║    1. claude plugin marketplace update
║         flow-marketplace
║    2. Start a new Claude Code session
║    3. Run /flow:flow-init
╚══════════════════════════════════════════════╝
```
````

### Step 2 — Check for existing feature

Use the Glob tool to check for existing state files matching `.flow-states/*.json`.

If any files are found, list their names (the branch names from the filenames).

If any files are found and continue=auto, print a warning and proceed automatically.

If any files are found and continue=manual, use AskUserQuestion:

> "An active FLOW feature already exists. What would you like to do?"
>
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

<HARD-GATE>
Do NOT proceed past Step 2 until the existing feature check is complete. If existing features are found and the user chooses Cancel, stop here.
</HARD-GATE>

### Step 3 — Verify main is green

Run `bin/flow ci` on main before creating any resources:

```bash
bin/flow ci
```

If it fails, stop immediately:

> "`bin/flow ci` is failing on main. Please fix CI before starting a new feature."

Do not create a worktree, PR, or state file. Exit the skill entirely.

### Step 4 — Set up workspace

Run the consolidated setup script:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>"
```

The script performs these operations in a single process:

1. `git pull origin main`
2. `git worktree add .worktrees/<branch> -b <branch>`
3. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
4. Create `.flow-states/<branch>.json` (initial state, all 6 phases)

The script logs each operation to `.flow-states/<branch>.log` internally.

**On success** — stdout is JSON:

```json
{"status": "ok", "worktree": ".worktrees/<branch>", "pr_url": "...", "pr_number": 123, "feature": "...", "branch": "..."}
```

Parse the JSON. Then cd into the worktree:

```bash
cd .worktrees/<branch>
```

The Bash tool persists working directory between calls, so all subsequent
commands run inside the worktree automatically. Do NOT repeat `cd .worktrees/`
in later steps — it would look for a nested `.worktrees/` that doesn't exist.

**On failure** — stdout is error JSON, details on stderr:

```json
{"status": "error", "step": "git_pull", "message": "..."}
```

If the script returns an error, read the stderr output for details, report
the failure to the user, and stop.

### Step 5 — Update dependencies

Use the Glob tool to check if `bin/dependencies` exists in the worktree.

If `bin/dependencies` exists, run it:

```bash
bin/dependencies
```

Then run CI to verify:

```bash
bin/flow ci
```

- **Passes** — continue to Step 7
- **Fails** — launch the CI fix sub-agent (see Step 6). Pass the full
  `bin/flow ci` output. After the sub-agent returns:
  - **Fixed** — continue to Step 7 (dependency changes + fixes committed together)
  - **Not fixed** — stop and report to the user what is failing

If `bin/dependencies` does not exist, skip to Done silently.

### Step 6 — CI fix sub-agent

When `bin/flow ci` fails in Step 5, launch a sub-agent to diagnose
and fix the failures. Use the Agent tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Fix bin/flow ci failures"`

Provide these instructions (fill in the worktree path and bin/flow ci output):

> You are fixing CI failures in a worktree.
> Worktree: `<worktree path>`
> cd into the worktree before running any commands.
>
> The `bin/flow ci` output:
> <paste the full bin/flow ci output>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions (bin/flow ci). Never use
> `cd <path> && git` — if you need git info, use `git -C <worktree_path>`.
> Never use Bash for any other purpose — no find, ls, cat, wc, test -f,
> stat, or running project tooling not listed here.
>
> Read the project CLAUDE.md for framework conventions, test commands,
> and CI failure fix order. Follow those instructions to diagnose and
> fix the failures.
>
> Max 3 attempts. After each fix, run `bin/flow ci`. If green, report what
> was fixed and stop. If still failing after 3 attempts, report exactly
> what is failing and what was tried.
>
> Return:
>
> 1. Status: fixed / not_fixed
> 2. What was wrong
> 3. What was changed (files modified)

Wait for the sub-agent to return.

<HARD-GATE>
Do NOT proceed past Step 5 until `bin/flow ci` is green.
</HARD-GATE>

### Step 7 — Commit and push

Use `/flow:flow-commit` to review and commit any dependency changes.

### Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-start --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 1: Start — COMPLETE (<formatted_time>)
============================================
```
````

Invoke the `flow:flow-status` skill to show the current state.

**If continue=auto**, skip the transition question and invoke `flow:flow-plan` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Plan?"
>
> - **Yes, start Phase 2 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 2 now" and "Not yet"

**If Yes** — invoke the `flow:flow-plan` skill using the Skill tool.

**If Not yet** — output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

Then report:
- Worktree location
- PR link
- Any additional report items from the framework section above

## Hard Rules

- Do not narrate internal operations to the user — no "The framework is Python", no "Proceeding to phase completion", no "No additional setup steps are needed". Just do the work silently and show results
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
