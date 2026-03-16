---
name: flow-start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades dependencies, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:flow-start <feature name words>"
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:flow-start invoice pdf export
/flow:flow-start --auto invoice pdf export
/flow:flow-start --manual invoice pdf export
```

**Feature name resolution:** Strip flags (`--auto`, `--manual`) from the arguments. ALL remaining words are the feature name — pass them through verbatim. Do not filter, rephrase, summarize, or ask the user to confirm. The `start-setup` script handles sanitization (special characters, casing, truncation) automatically.

Words are joined with hyphens:

- Branch: `invoice-pdf-export`
- Worktree: `.worktrees/invoice-pdf-export`
- PR title: `Invoice Pdf Export`

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen. Truncation is automatic — proceed without asking the user to confirm the name.

<HARD-GATE>
Do NOT proceed if no arguments were provided after the command (excluding flags).
Output this error message and stop:

> "Feature name required. Usage: `/flow:flow-start <feature name words>`"

No interactive prompt. The user re-runs the command with arguments.
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
  FLOW v0.31.1 — Phase 1: Start — STARTING
============================================
```
````

## Logging

After every Bash command in Steps 3–4, log it to `.flow-states/<branch>.log`
using `bin/flow log`. Step 2 handles its own logging internally.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step X — desc (exit EC)"
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

### Step 1 — Pre-flight checks

Run all four in parallel (one response, multiple tool calls):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow prime-check
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow upgrade-check
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Also use the Glob tool: pattern `*.json`, path `.flow-states` — checks for existing active features.

Process the results in this order:

**1a. Version gate (prime-check):**

- If `"status": "error"` — show the error message from the JSON (it suggests `/flow:flow-prime --reprime` or `/flow:flow-prime`) and stop. Do not proceed to any further steps.
- If `"status": "ok"` and `"auto_upgraded": true` — show this notice using the `old_version` and `new_version` fields from the JSON, then continue:

````markdown
```text
FLOW auto-upgraded from v{old_version} to v{new_version} (config unchanged).
```
````

- If `"status": "ok"` without `auto_upgraded` — proceed silently.

<HARD-GATE>
Do NOT proceed if version check fails. Show the error message and stop.
</HARD-GATE>

**1b. Upgrade check:**

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
║    3. Run /flow:flow-prime
╚══════════════════════════════════════════════╝
```
````

**1c. Existing feature check (Glob results):**

If any state files are found, list their names (the branch names from the filenames).

If any files are found and continue=auto, print a warning and proceed automatically.

If any files are found and continue=manual, use AskUserQuestion:

> "An active FLOW feature already exists. What would you like to do?"
>
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

<HARD-GATE>
Do NOT proceed past the existing feature check. If existing features are found and the user chooses Cancel, stop here.
</HARD-GATE>

**1d. CI result:**

If CI passed, continue to Step 2.

If it fails, launch the `ci-fixer` sub-agent to diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures on main"`

Provide the full `bin/flow ci` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — commit the fixes via `/flow:flow-commit --auto`, then continue to Step 2
- **Not fixed** — stop and report to the user. Do not create a worktree, PR, or state file

<HARD-GATE>
Do NOT proceed to Step 2 until the ci-fixer changes are committed and pushed via `/flow:flow-commit --auto`. Uncommitted fixes on main will not appear in the worktree.
</HARD-GATE>

### Step 2 — Set up workspace

Run the consolidated setup script:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>" --prompt "<full-start-prompt>"
```

`<full-start-prompt>` is the user's original input verbatim, including `#N` issue references and any special characters. Do not sanitize or transform it.

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

Parse the JSON. Then run both in parallel (one response, two tool calls):

```bash
cd .worktrees/<branch>
```

Also use the Read tool to check if `bin/dependencies` exists at `<project_root>/.worktrees/<branch>/bin/dependencies`.

The Bash tool persists working directory between calls, so all subsequent
commands run inside the worktree automatically. Do NOT repeat `cd .worktrees/`
in later steps — it would look for a nested `.worktrees/` that doesn't exist.

If Read returns an error (file not found), skip to Done silently.

**On failure** — stdout is error JSON, details on stderr:

```json
{"status": "error", "step": "git_pull", "message": "..."}
```

If the script returns an error, read the stderr output for details, report
the failure to the user, and stop.

### Step 3 — Update dependencies

If `bin/dependencies` was found in Step 2, run it:

```bash
bin/dependencies
```

Then run the log and CI in parallel (one response, two Bash calls):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step 3 — bin/dependencies (exit EC)"
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Use the exit code from the `bin/dependencies` command for EC in the log entry.

- **CI passes** — continue to Step 4
- **CI fails** — launch the `ci-fixer` sub-agent to diagnose and fix.
  Use the Agent tool:
  - `subagent_type`: `"flow:ci-fixer"`
  - `description`: `"Fix bin/flow ci failures"`
  - Provide the full `bin/flow ci` output in the prompt.
  - After the sub-agent returns:
    - **Fixed** — continue to Step 4 (dependency changes + fixes committed together)
    - **Not fixed** — stop and report to the user what is failing

<HARD-GATE>
Do NOT proceed past Step 3 until `bin/flow ci` is green.
</HARD-GATE>

### Step 4 — Commit and push

Run the CI log and git status in parallel (one response, two Bash calls):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step 3 — bin/flow ci (exit EC)"
```

```bash
git status
```

Use the exit code from the `bin/flow ci` command for EC in the log entry.

Then log the status result:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step 4 — git status (exit EC)"
```

If `git status` shows no uncommitted changes, skip directly to Done.

Otherwise, use `/flow:flow-commit --auto` to review and commit any dependency changes. No exceptions. Never use `git commit` directly.

### Done — Update state and complete phase

Complete the phase:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-start --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.31.1 — Phase 1: Start — COMPLETE (<formatted_time>)
============================================
```
````

**If continue=auto**, invoke `flow:flow-plan` directly. Do not invoke
`flow:flow-status` or use AskUserQuestion.

**If continue=manual**:

Invoke the `flow:flow-status` skill to show the current state.

Use AskUserQuestion:

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
