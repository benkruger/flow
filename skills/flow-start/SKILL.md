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

**Issue-aware branch naming:** When the prompt contains `#N` issue
references (e.g., `work on issue #309`, `fix #42`), `init-state`
(Step 3) fetches the first issue's title and derives the branch name
from it. If the fetch fails, init-state returns a hard error — there
is no silent fallback to the prompt words. Capture the `branch` field
from init-state's JSON output and use it for all subsequent steps.

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
  FLOW v1.1.0 — Phase 1: Start — STARTING
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

<HARD-GATE>
When the lock status is "locked", the ONLY permitted action is to invoke
the loop skill as described above. The start-lock command has built-in
staleness detection (30-minute timeout) that handles genuinely dead sessions.

Do NOT speculate about whether the lock is stale.
Do NOT offer to release, reset, or clean up the lock.
Do NOT suggest any workaround that bypasses the lock.
Do NOT take any action other than invoking the loop skill and returning.

Trust the tool output. Poll and wait.

</HARD-GATE>

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

- If `"status": "error"` — release the lock, show the error message from the JSON (it suggests `/flow:flow-prime --reprime` or `/flow:flow-prime`), and stop. This is a flow-specific error — main is untouched, so the next queued flow can proceed.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release --feature <feature-name>
```

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
${CLAUDE_PLUGIN_ROOT}/bin/flow init-state "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt --start-step 3 --start-steps-total 11
```

If `--auto` was passed to this skill invocation, also pass `--auto`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow init-state "<feature-name>" --prompt-file .flow-states/<feature-name>-start-prompt --auto --start-step 3 --start-steps-total 11
```

Parse the JSON output. If `"status": "error"`, release the lock, report
the error, and stop. These are flow-specific errors — main is untouched,
so the next queued flow can proceed.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release --feature <feature-name>
```

If `"step"` is `"fetch_issue_title"`, the issue title could not be fetched.
If `"step"` is `"duplicate_issue"`, another flow already targets the same
issue. In both cases, the lock release above already ran — report the
error to the user and stop.

On success, capture the `branch` field from the JSON output. This is the
**canonical branch name** — it may differ from `<feature-name>` when the
prompt contains issue references (e.g., `<feature-name>` is
`work-on-issue-309` but `branch` is `organize-settings-allow-list`).
Use this canonical branch for all `--branch` flags in Steps 4–10.

### Step 4 — Label referenced issues

If the start prompt contains `#N` issue references, add the "Flow In-Progress"
label so other engineers can see these issues are being worked on:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 4 --branch <branch> -- label-issues --state-file <project_root>/.flow-states/<branch>.json --add
```

Best-effort — if labeling fails, log the result and continue. Do not block
the Start phase for a label failure.

### Step 5 — Pull latest main

Run both in parallel (one response, two Bash calls):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 5 --branch <branch>
```

```bash
git pull origin main
```

### Step 6 — CI baseline gate

Main is pristine — nothing merges without clean CI. A single-attempt
failure is likely flaky and is retried. Consistent failure (all 3
retries) indicates real main breakage.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 6 --branch <branch> -- ci --retry 3 --branch main
```

Parse the JSON output:

**If `status` is `"ok"` and `flaky` is absent** — CI passed cleanly.
Continue to Step 7.

**If `status` is `"ok"` and `flaky` is `true`** — a test failed then
passed on retry. File a "Flaky Test" issue with reproduction data from
the `first_failure_output` field, then continue to Step 7.

The issue body must include: the failure output from `first_failure_output`,
how many attempts it took to pass (from the `attempts` field), and the
context "CI baseline on pristine main during flow-start".

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-start"
```

**If `status` is `"error"` and `consistent` is `true`** — all 3
attempts failed. Hold the lock and stop. Main is broken — the next
queued flow would hit the same failure. Report to the user that CI is
consistently failing on pristine main. The 30-minute stale timeout
releases the lock if the user does not act.

### Step 7 — Update dependencies

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 7 --branch <branch>
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow update-deps
```

Parse the JSON output:

- If `status` is `"skipped"` → skip to Step 10 (release lock).
- If `status` is `"ok"` and `changes` is `false` → skip to Step 10 (release lock).
- If `status` is `"ok"` and `changes` is `true` → continue to Step 8.
- If `status` is `"error"` → release the lock and stop. The dependency tool failed before modifying main (timeout, network, exec error) — main is untouched, so the next queued flow can proceed. Report the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-lock --release --feature <feature-name>
```

### Step 8 — CI post-deps gate

If dependencies changed anything, run CI again to catch dep-induced breakage
(rubocop violations, breaking changes, etc.):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 8 --branch <branch> -- ci --retry 3 --branch main
```

Parse the JSON output:

**If `status` is `"ok"` and `flaky` is absent** — CI passed cleanly.
Continue to Step 9.

**If `status` is `"ok"` and `flaky` is `true`** — a test failed then
passed on retry. File a "Flaky Test" issue with reproduction data from
the `first_failure_output` field, then continue to Step 9.

The issue body must include: the failure output from `first_failure_output`,
how many attempts it took to pass (from the `attempts` field), and the
context "CI post-deps gate during flow-start after dependency update".

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-start"
```

**If `status` is `"error"` and `consistent` is `true`** — all 3
attempts failed consistently. This is real dep-induced breakage.
Launch the `ci-fixer` sub-agent to diagnose and fix. Use the Agent
tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures after dependency update"`

Provide the CI output from the `output` field in the prompt so the
sub-agent knows what failed.

Wait for the sub-agent to return.

- **Fixed** — continue to Step 9
- **Not fixed** — hold the lock and stop. Main has uncommitted dep-induced breakage — the next queued flow would hit the same failure. Report to the user.

### Step 9 — Commit to main

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 9 --branch <branch>
```

If there are any uncommitted changes (dependency updates + CI fixes),
commit them to main via `/flow:flow-commit --auto`.

### Step 10 — Release start lock

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 10 --branch <branch> -- start-lock --release --feature <feature-name>
```

<HARD-GATE>
Do NOT proceed to Step 11 until the lock is released and `bin/flow ci` is green.
Uncommitted fixes on main will not appear in the worktree.
</HARD-GATE>

### Step 11 — Set up workspace

Write the user's original start prompt (verbatim, including `#N` issue references
and any special characters) to `.flow-states/<branch>-start-prompt` using the
Write tool. Then run the setup script. If `--auto` was passed to this skill
invocation, also pass `--auto` to the start-setup command:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 11 --branch <branch> -- start-setup "<feature-name>" --branch <branch> --prompt-file .flow-states/<branch>-start-prompt --skip-pull
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-step --step 11 --branch <branch> -- start-setup "<feature-name>" --branch <branch> --prompt-file .flow-states/<branch>-start-prompt --skip-pull --auto
```

Use the first form when no mode flag was passed or `--manual` was passed.
Use the second form when `--auto` was passed.

The script reads the prompt file and deletes it automatically after reading.
The `--branch` flag passes the canonical branch from init-state (Step 3)
directly, so start-setup does not need to scan for the state file.

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
  ✓ FLOW v1.1.0 — Phase 1: Start — COMPLETE (<formatted_time>)
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
STOP. Parse `continue_action` from the `phase-transition --action complete`
output above to determine how to advance.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use `continue_action` from the phase-transition output.
   If `continue_action` is `"invoke"` → continue=auto.
   If `continue_action` is `"ask"` → continue=manual.
2. If continue=auto → invoke `flow:flow-plan` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
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
  Run /flow:flow-plan when ready.
══════════════════════════════════════════════════
```
````

## Hard Rules

- Do not narrate internal operations to the user — no "The framework is Python", no "Proceeding to phase completion", no "No additional setup steps are needed". Just do the work silently and show results
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
