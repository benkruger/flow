---
name: flow-start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades dependencies, opens a PR, creates .flow-states/<branch>/state.json, and configures the workspace. Usage: /flow:flow-start <feature name words>"
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:flow-start invoice pdf export
/flow:flow-start --auto invoice pdf export
/flow:flow-start --manual invoice pdf export
```

**Feature name resolution:** Strip flags (`--auto`, `--manual`) from the arguments. The remaining text is the **prompt** — a description of what to build. Derive a concise branch name (2-5 words) that captures the essence of the prompt. The `start-init` command handles sanitization (special characters, casing, truncation) automatically via `init-state`.

Examples:

| Prompt | Derived branch name |
|--------|-------------------|
| `invoice pdf export` | `invoice-pdf-export` |
| `fix login timeout when session expires after 30 minutes` | `fix-login-timeout` |
| `there is a bug where flow-start treats arguments as conversation` | `flow-start-arg-handling` |

**Issue-aware branch naming:** When the prompt contains `#N` issue
references (e.g., `work on issue #309`, `fix #42`), `start-init`
fetches the first issue's title and derives the branch name
from it. If the fetch fails, start-init returns a hard error — there
is no silent fallback to the prompt words. Capture the `branch` field
from start-init's JSON output and use it for all subsequent steps.

If the referenced issue already carries the "Flow In-Progress" label,
start-init also returns a hard error — the issue is already being worked
on by another flow (on this machine or another engineer's machine). The
user should resume the existing flow in its worktree, or reference a
different issue.

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
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

1. If `--auto` was passed → continue=auto AND override ALL skills to fully autonomous (all commits auto, all continues auto). The `--auto` flag is passed through to `start-init`, which writes the autonomous preset to the state file. All downstream phases inherit the override automatically.
2. If `--manual` was passed → continue=manual
3. Otherwise → resolved in the Done section by reading `skills.flow-start.continue` from `.flow-states/<branch>/state.json` (which exists after Step 1)

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

All four consolidated commands (`start-init`, `start-gate`, `start-workspace`,
`phase-finalize`) handle logging internally via `append_log()` to
`.flow-states/<branch>/log`. No model-level logging calls are needed.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 1] ..."
```

The bash block above is for reference only — all four commands call
`append_log()` internally. Do not run `bin/flow log` manually.

---

## Steps

### Step 1 — Initialize (lock, version checks, state file, labels)

Write the user's original start prompt (verbatim, including `#N` issue references
and any special characters) to `.flow-states/<feature-name>-start-prompt` using the
Write tool. Then run start-init. If `--auto` was passed, also pass `--auto`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-init <feature-name> --prompt-file .flow-states/<feature-name>-start-prompt
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-init <feature-name> --prompt-file .flow-states/<feature-name>-start-prompt --auto
```

Use the first form when no mode flag was passed or `--manual` was passed.
Use the second form when `--auto` was passed.

Parse the JSON output and branch on `status`:

**If `"status": "ready"`** — capture the `branch` field. This is the
**canonical branch name** — use it for all subsequent steps.

If `auto_upgraded` is `true`, show this notice using the `old_version` and
`new_version` fields:

````markdown
```text
FLOW auto-upgraded from v{old_version} to v{new_version} (config unchanged).
```
````

If `upgrade` is present and `upgrade.status` is `"upgrade_available"`, show
the upgrade notice:

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

Continue to Step 2.

**If `"status": "locked"`** — another start holds the lock. Invoke the `loop`
skill via the Skill tool with args `15s /flow:flow-start` and return.
The loop re-invokes the entire skill every 15 seconds. Since nothing has
executed yet, re-running is safe. When the lock is eventually acquired,
the skill proceeds through all steps normally.

<HARD-GATE>
When the status is "locked", the ONLY permitted action is to invoke
the loop skill as described above. The start-init command has built-in
staleness detection (30-minute timeout) that handles genuinely dead sessions.

Do NOT speculate about whether the lock is stale.
Do NOT offer to release, reset, or clean up the lock.
Do NOT suggest any workaround that bypasses the lock.
Do NOT take any action other than invoking the loop skill and returning.

Trust the tool output. Poll and wait.

</HARD-GATE>

**If `"status": "error"`** — show the error message and stop. start-init
has already released the lock for flow-specific errors. Common error steps
include `fetch_issue_title` (issue not found), `flow_in_progress_label`
(issue already being worked on), and `duplicate_issue` (another flow
targets the same issue).

<HARD-GATE>
Do NOT proceed if version check fails. Show the error message and stop.
</HARD-GATE>

### Step 2 — CI and dependency gate

Use a 10-minute Bash tool timeout (`timeout: 600000`) — CI runs can
take 3–4 minutes and the default 2-minute timeout would background
the process, defeating the gate (per `.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-gate --branch <branch>
```

Parse the JSON output and branch on `status`:

**If `"status": "clean"`** — all gates passed. Continue to Step 3.

**If `"status": "ci_flaky"`** — a test failed then passed on retry.
File a "Flaky Test" issue with reproduction data from the `first_failure_output`
and `attempts` fields, using the `flaky_context` field for the issue body context.

Write the issue body to `.flow-states/<branch>/issue-body-content.md` using
the Write tool, then route it to `.flow-issue-body` in the project root
via `bin/flow write-rule` (avoids Claude Code's Write-tool preflight on a
pre-existing body file — see `.claude/rules/file-tool-preflights.md`):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <project_root>/.flow-issue-body --content-file .flow-states/<branch>/issue-body-content.md
```

Then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-start"
```

Then continue to Step 3.

**If `"status": "ci_failed"`** — all retry attempts failed consistently.
Hold the lock and stop. The integration branch is broken — the next
queued flow would hit the same failure. Report to the user that CI is
consistently failing on the pristine integration branch. The 30-minute
stale timeout releases the lock if the user does not act.

**If `"status": "deps_ci_failed"`** — dependencies were updated but
post-deps CI failed consistently. Launch the `ci-fixer` sub-agent to
diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix bin/flow ci failures after dependency update"`

Provide the CI output from the `output` field in the prompt so the
sub-agent knows what failed.

Wait for the sub-agent to return.

- **Fixed** — commit CI fixes to the integration branch via `/flow:flow-commit`, then continue to Step 3
- **Not fixed** — hold the lock and stop. The integration branch has uncommitted dep-induced breakage. Report to the user.

**If `"status": "error"`** — show the error message and stop.

### Step 3 — Create workspace (worktree, PR, lock release)

Write the user's original start prompt (verbatim, including `#N` issue references
and any special characters) to `.flow-states/<branch>/start-prompt` using the
Write tool. Then run start-workspace:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow start-workspace "<feature-name>" --branch <branch> --prompt-file .flow-states/<branch>/start-prompt
```

The command creates the worktree, opens a PR, backfills the state file with
PR fields, and releases the start lock as its final action.

**On success** — parse the JSON output. Capture the `worktree_cwd`
field — this is the directory the agent should cd into. For root-level
flows it equals `.worktrees/<branch>`; for flows started from inside a
mono-repo subdirectory (`relative_cwd` non-empty) it includes the
subdirectory suffix (e.g. `.worktrees/<branch>/api`). Then run:

```bash
cd <worktree_cwd>
```

Substitute the literal `worktree_cwd` value from the JSON response. The
Bash tool persists working directory between calls, so all subsequent
commands run inside that directory automatically. Do NOT repeat
`cd .worktrees/` in later steps — it would look for a nested
`.worktrees/` that doesn't exist.

After the cd, every `bin/flow` subcommand enforces this directory via
its built-in cwd-drift guard. If you cd elsewhere within the worktree
(e.g. into a sibling subdirectory), the next subcommand will hard-error
with an "expected directory" message.

**On failure** — report the error and stop. The command releases the lock
even on error (main is untouched by worktree operations).

### Step 4 — Change to worktree

This step is the `cd` from Step 3. The TUI shows Step 4 while the
worktree directory is active and phase-finalize runs in Step 5.

### Step 5 — Update state and finalize (complete phase, notify)

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-finalize --phase flow-start --branch <branch> --pr-url <pr_url>
```

The command runs `phase_complete()` internally, updates the state file,
and sends Slack notifications. Parse the JSON output. Use the
`formatted_time` field in the COMPLETE banner below. Do not print the
timing calculation. Use the `continue_action` field for the transition
HARD-GATE.

### Done — Banner and transition

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 1: Start — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

<HARD-GATE>
STOP. Parse `continue_action` from the `phase-finalize` output above
to determine how to advance.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use `continue_action` from the `phase-finalize` output.
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
      location and PR link
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

- Do not narrate internal operations to the user — no "Proceeding to phase completion", no "No additional setup steps are needed". Just do the work silently and show results
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
