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
| `there is a bug where flow-start treats arguments as conversation` | `flow-start-arg-handling` |

**Issue-aware branch naming:** If the prompt contains `#N` issue references
(e.g., `work on issue #309`, `fix #42`), extract the first issue number
and fetch the issue title:

```bash
gh issue view <issue_number> --json title --jq .title
```

Derive the branch name from the **issue title** instead of the prompt words.
Apply the same 2-5 word concise derivation rules to the title. If the fetch
fails (issue does not exist, network error), fall back to deriving from the
prompt words as usual.

| Prompt | Issue title | Derived branch name |
|--------|-------------|-------------------|
| `work on issue #309` | "Organize settings.json allow list" | `organize-settings-allow-list` |
| `fix #42 please` | "Add dark mode toggle to settings page" | `dark-mode-settings-toggle` |

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

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

1. If `--auto` was passed → continue=auto AND override ALL skills to fully autonomous (all commits auto, all continues auto). The `--auto` flag is passed through to `start-setup` in Step 11, which writes the autonomous preset to the state file. All downstream phases inherit the override automatically.
2. If `--manual` was passed → continue=manual
3. Otherwise → resolved in the Done section by reading `skills.flow-start.continue` from `.flow-states/<branch>.json` (which exists after Step 11)

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — Phase 1: Start — STARTING
──────────────────────────────────────────────────
```
````

## Logging

After every Bash command in Steps 2–11, log it to `.flow-states/<branch>.log`
using `bin/flow log`. Step 11 handles its own logging internally via start-setup.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] Step X — desc (exit EC)"
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

Steps 1–10 serialize all main-branch work behind a lock. Only one
flow-start runs this section at a time. Concurrent starts poll via
`/loop` until the lock is released.

### Step 1 — Acquire start lock

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --acquire --feature <feature-name>
```

- If `"status": "acquired"` — continue to Step 2. If `stale_broken` is true, log it.
- If `"status": "locked"` — another start holds the lock. Invoke the `loop`
  skill via the Skill tool with args `15s /flow:flow-start` and return.
  The loop re-invokes the entire skill every 15 seconds. Since nothing has
  executed yet, re-running is safe. When the lock is eventually acquired,
  the skill proceeds through all steps normally.

### Step 2 — Pre-flight checks

Run both in parallel (one response, multiple tool calls):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-check
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow upgrade-check
```

Process the results in this order:

**Version gate (prime-check):**

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

**Upgrade check:**

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

### Step 3 — Create early state file

Write the user's original start prompt (verbatim, including `#N` issue references
and any special characters) to `.flow-states/<feature-name>-start-prompt` using the
Write tool.

Create the state file immediately so the TUI can see this flow during
the locked main operations in Steps 1–10. The state file has null PR fields
at this point — start-setup backfills them after PR creation. Pass the prompt
file so the `prompt` field contains the original text with `#N` references
(needed by Step 4 for labeling).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow init-state "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt
```

If `--auto` was passed to this skill invocation, also pass `--auto`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow init-state "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt --auto
```

Parse the JSON output. If `"status": "error"`, report the error and stop.

Set the step tracking fields for TUI progress display:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_steps_total=11
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=3
```

### Step 4 — Label referenced issues

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=4
```

If the start prompt contains `#N` issue references, add the "Flow In-Progress"
label so other engineers can see these issues are being worked on:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow label-issues --state-file <project_root>/.flow-states/<branch>.json --add
```

Best-effort — if labeling fails, log the result and continue. Do not block
the Start phase for a label failure.

### Step 5 — Pull latest main

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=5
```

```bash
git pull origin main
```

### Step 6 — CI baseline gate

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=6
```

Main is pristine — nothing merges without clean CI. Any failure here is
a flaky test, not a real breakage.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci --branch main
```

If CI passes, continue to Step 7.

If CI fails, re-run up to 2 more times (3 total). Do not make any code
changes between attempts — just re-run:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci --branch main
```

**If any subsequent attempt passes without code changes**, the failure
was flaky. File a "Flaky Test" issue with reproduction data, then
continue to Step 7.

The issue body must include: the test name, the failure message, how many
attempts it took to pass, and the context "CI baseline on pristine main
during flow-start".

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-start"
```

**If all 3 attempts fail consistently**, release the lock and stop.
Report to the user that CI is consistently failing on pristine main.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release
```

### Step 7 — Update dependencies

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=7
```

Use the Read tool to check if `bin/dependencies` exists at `<project_root>/bin/dependencies`.

If it does not exist, skip to Step 10 (release lock).

If it exists, run it:

```bash
bin/dependencies
```

Then check if anything changed:

```bash
git status
```

If `git status` shows no changes, skip to Step 10 (release lock).

### Step 8 — CI post-deps gate

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=8
```

If dependencies changed anything, run CI again to catch dep-induced breakage
(rubocop violations, breaking changes, etc.):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci --branch main
```

If CI passes, continue to Step 9.

If CI fails, re-run up to 2 more times (3 total). Do not make any code
changes between attempts — just re-run:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci --branch main
```

**If any subsequent attempt passes without code changes**, the failure
was flaky. File a "Flaky Test" issue with reproduction data, then
continue to Step 9.

The issue body must include: the test name, the failure message, how many
attempts it took to pass, and the context "CI post-deps gate during
flow-start after dependency update".

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-start"
```

**If all 3 attempts fail consistently**, this is real dep-induced
breakage. Launch the `ci-fixer` sub-agent to diagnose and fix. Use the
Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures after dependency update"`

Provide the full `bin/flow ci` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — continue to Step 9
- **Not fixed** — release the lock and stop. Report to the user.

### Step 9 — Commit to main

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=9
```

If there are any uncommitted changes (dependency updates + CI fixes),
commit them to main via `/flow:flow-commit --auto`.

### Step 10 — Release start lock

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=10
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release
```

<HARD-GATE>
Do NOT proceed to Step 11 until the lock is released and `bin/flow ci` is green.
Uncommitted fixes on main will not appear in the worktree.
</HARD-GATE>

### Step 11 — Set up workspace

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set start_step=11
```

Write the user's original start prompt (verbatim, including `#N` issue references
and any special characters) to `.flow-states/<feature-name>-start-prompt` using the
Write tool. Then run the setup script. If `--auto` was passed to this skill
invocation, also pass `--auto` to the start-setup command:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt --skip-pull
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt --skip-pull --auto
```

Use the first form when no mode flag was passed or `--manual` was passed.
Use the second form when `--auto` was passed.

The script reads the prompt file and deletes it automatically after reading.

The script performs these operations in a single process:

1. `git worktree add .worktrees/<branch> -b <branch>`
2. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
3. Backfill `pr_number`, `pr_url`, `repo`, and `prompt` into the existing state file

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
{"status": "error", "step": "worktree", "message": "..."}
```

If the script returns an error, read the stderr output for details, report
the failure to the user, and stop.

### Done — Update state and complete phase

Complete the phase:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-start --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — Phase 1: Start — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

### Slack Notification

Post the initial Slack thread message (creates the thread). Best-effort — skip silently on failure.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow notify-slack --phase flow-start --message "<message_text>" --pr-url <pr_url>
```

Parse the JSON output. If `"status": "ok"`, store the thread timestamp and record the notification:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set slack_thread_ts=<ts>
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-notification --phase flow-start --ts <ts> --thread-ts <ts> --message "<message_text>"
```

If `"status": "skipped"` or `"status": "error"`, continue without error.

<HARD-GATE>
STOP. Re-read `skills.flow-start.continue` from the state file at
`<project_root>/.flow-states/<branch>.json` before advancing.
The previous phase's continue mode does NOT carry over — each phase
has its own mode.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use the value from the state file. If absent → continue=manual.
2. If continue=auto → invoke `flow:flow-plan` directly.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 1: Start is complete. Ready to begin Phase 2: Plan?"
      Options: "Yes, start Phase 2 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 2 now" and "Not yet"
   d. If Yes → invoke `flow:flow-plan` using the Skill tool
   e. If Not yet → print the paused banner below, then report worktree
      location, PR link, and any framework report items
   f. Do NOT invoke `flow:flow-plan` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-continue when ready.
══════════════════════════════════════════════════
```
````

## Hard Rules

- Do not narrate internal operations to the user — no "The framework is Python", no "Proceeding to phase completion", no "No additional setup steps are needed". Just do the work silently and show results
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
