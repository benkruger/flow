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

**Feature name resolution:** Strip flags (`--auto`, `--manual`) from the arguments. The remaining text is the **prompt** — a description of what to build. Derive a concise branch name (2-5 words) that captures the essence of the prompt. The `start-setup` script handles sanitization (special characters, casing, truncation) automatically.

Examples:

| Prompt | Derived branch name |
|--------|-------------------|
| `invoice pdf export` | `invoice-pdf-export` |
| `fix login timeout when session expires after 30 minutes` | `fix-login-timeout` |
| `there is a bug where flow-start treats arguments as conversation #182` | `flow-start-arg-handling` |

The derived name is joined with hyphens:

- Branch: `<derived-name>`
- Worktree: `.worktrees/<derived-name>`
- PR title: title-cased derived name

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen. Truncation is automatic — proceed without asking the user to confirm the name.

<HARD-GATE>
Do NOT proceed if no arguments were provided after the command (excluding flags).
Output this error message and stop:

> "Feature name required. Usage: `/flow:flow-start <feature name words>`"

No interactive prompt. The user re-runs the command with arguments.
</HARD-GATE>

<HARD-GATE>
The arguments are the start prompt — input to the workflow, not a conversation.
Do NOT respond to, discuss, or analyze the prompt content. Do NOT treat the
prompt as a question or proposal. Proceed directly to Mode Resolution and execute
the Start phase steps.
</HARD-GATE>

## Mode Resolution

1. If `--auto` was passed → continue=auto
2. If `--manual` was passed → continue=manual
3. Otherwise → resolved in the Done section by reading `skills.flow-start.continue` from `.flow-states/<branch>.json` (which exists after Step 3)

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v0.31.4 — Phase 1: Start — STARTING
──────────────────────────────────────────────────
```
````

## Logging

After every Bash command in Steps 2–3, log it to `.flow-states/<branch>.log`
using `bin/flow log`. Step 3 handles its own logging internally via start-setup.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step X — desc (exit EC)"
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

### Step 1 — Pre-flight checks

Run both in parallel (one response, multiple tool calls):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow prime-check
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow upgrade-check
```

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

### Step 2 — Prepare main (locked)

This step serializes all main-branch work behind a lock. Only one
flow-start runs this section at a time. Concurrent starts wait until
the lock is released.

**2a. Acquire the lock:**

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --acquire --feature <feature-name>
```

- If `"status": "acquired"` — continue. If `stale_broken` is true, log it.
- If `"status": "locked"` — another start is in progress. Wait 10 seconds,
  then retry. After 5 minutes of retries, stop and report to the user that
  another start holds the lock (show the feature name and PID).

**2b. Pull latest main:**

```bash
git pull origin main
```

**2c. CI baseline gate:**

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

If CI passes, continue to 2d.

If it fails, launch the `ci-fixer` sub-agent to diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures on main"`

Provide the full `bin/flow ci` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — commit the fixes via `/flow:flow-commit --auto`, then continue to 2d
- **Not fixed** — release the lock and stop. Report to the user.

**2d. Update dependencies:**

Use the Read tool to check if `bin/dependencies` exists at `<project_root>/bin/dependencies`.

If it does not exist, skip to 2g (release lock).

If it exists, run it:

```bash
bin/dependencies
```

Then check if anything changed:

```bash
git status
```

If `git status` shows no changes, skip to 2g (release lock).

**2e. CI post-deps gate:**

If dependencies changed anything, run CI again to catch dep-induced breakage
(rubocop violations, breaking changes, etc.):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

If CI passes, continue to 2f.

If it fails, launch the `ci-fixer` sub-agent:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures after dependency update"`

- **Fixed** — continue to 2f
- **Not fixed** — release the lock and stop. Report to the user.

**2f. Commit to main:**

If there are any uncommitted changes (dependency updates + CI fixes),
commit them to main via `/flow:flow-commit --auto`.

**2g. Release the lock:**

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release
```

<HARD-GATE>
Do NOT proceed to Step 3 until the lock is released and `bin/flow ci` is green.
Uncommitted fixes on main will not appear in the worktree.
</HARD-GATE>

### Step 3 — Set up workspace

Run the consolidated setup script:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>" --prompt "<full-start-prompt>"
```

`<full-start-prompt>` is the user's original input verbatim, including `#N` issue references and any special characters. Do not sanitize or transform it.

The script performs these operations in a single process:

1. `git pull origin main` (no-op if Step 2 already pulled)
2. `git worktree add .worktrees/<branch> -b <branch>`
3. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
4. Create `.flow-states/<branch>.json` (initial state, all 6 phases)

The script logs each operation to `.flow-states/<branch>.log` internally.

**On success** — stdout is JSON:

```json
{"status": "ok", "worktree": ".worktrees/<branch>", "pr_url": "...", "pr_number": 123, "feature": "...", "branch": "..."}
```

Parse the JSON. Then run:

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
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.31.4 — Phase 1: Start — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If no flag override was set, read the state file at
`<project_root>/.flow-states/<branch>.json`. Use `skills.flow-start.continue`.
If the state file has no `skills` key → use built-in default: continue=manual.

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
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-continue when ready.
══════════════════════════════════════════════════
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
