---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades gems, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:start <feature name words>"
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:start invoice pdf export
```

Arguments become the feature name. Words are joined with hyphens:

- Branch: `invoice-pdf-export`
- Worktree: `.worktrees/invoice-pdf-export`
- PR title: `Invoice Pdf Export`

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen.

<HARD-GATE>
Do NOT proceed if the feature name is missing. Ask the user:
"What is the feature name? e.g. /flow:start invoice pdf export"
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.8.1 — Phase 1: Start — STARTING
  Recommended model: Haiku
============================================
```
````

## Logging

After every Bash command in Steps 3–7, log it to `.flow-states/<branch>.log`. Step 2 handles its own logging internally.

Run the command with exit code capture:

```bash
COMMAND; EC=$?; exit $EC
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 1] Step X — desc (exit EC)
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

### Step 1 — Check for existing feature

Use the Glob tool to check for existing state files matching `.flow-states/*.json`.

If any files are found, list their names (the branch names from the filenames).

If any files are found, use AskUserQuestion:

> "An active FLOW feature already exists. What would you like to do?"
>
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

### Step 2 — Set up workspace

Run the consolidated setup script:

```bash
python3 ${CLAUDE_PLUGIN_ROOT}/hooks/start-setup.py "<feature-name>"
```

The script performs these operations in a single process:

1. Verify `/flow:init` has been run (version gate)
2. `git pull origin main`
3. `git worktree add .worktrees/<branch> -b <branch>`
4. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
5. Create `.flow-states/<branch>.json` (initial state, all 8 phases)

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

If the error step is `init_check`, tell the user to run `/flow:init` first
and stop. For all other errors, read the stderr output for details, report
the failure to the user, and stop.

### Step 3 — Baseline `bin/ci`

```bash
bin/ci
```

- **Passes** — note as baseline and continue
- **Fails** — launch the CI fix sub-agent (see Step 6). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — use `/flow:commit` to commit the fix, then continue
  - **Not fixed** — stop and report to the user what is failing

### Step 4 — Upgrade gems

```bash
bundle update --all
```

### Step 5 — Post-update `bin/ci`

```bash
bin/ci
```

- **Passes** — continue to Step 7
- **Fails** — launch the CI fix sub-agent (see Step 6). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — continue to Step 7 (Gemfile.lock + fixes committed together)
  - **Not fixed** — stop and report to the user what is failing

### Step 6 — CI fix sub-agent

When `bin/ci` fails in Step 3 or Step 5, launch a sub-agent to diagnose
and fix the failures. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `model`: `"sonnet"`
- `description`: `"Fix bin/ci failures"`

Provide these instructions (fill in the worktree path and bin/ci output):

> You are fixing CI failures in a Rails worktree.
> Worktree: `<worktree path>`
> cd into the worktree before running any commands.
>
> The `bin/ci` output:
> <paste the full bin/ci output>
>
> Fix the failures in this order:
>
> 1. **RuboCop violations** — run `rubocop -A` to auto-fix, then `bin/ci`
> 2. **Test failures** — read the failing test and the code it tests.
>    Understand the root cause. Fix the code, not the test (unless the
>    test itself is wrong). Run `bin/rails test <file>` to verify,
>    then `bin/ci` for a full check.
> 3. **Coverage gaps** — read `test/coverage/uncovered.txt` to see exactly
>    which lines are uncovered. Write the missing test, then `bin/ci`
>
> Max 3 attempts. After each fix, run `bin/ci`. If green, report what
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
Do NOT proceed past Step 3 or Step 5 until bin/ci is green.
</HARD-GATE>

### Step 7 — Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` + any gem fixes).

### Done — Update state and complete phase

Update `.flow-states/<branch>.json`:
1. `cumulative_seconds` for Phase 1: `current_time - session_started_at`. Do not print the calculation.
2. Phase 1 `status` → `complete`
3. Phase 1 `completed_at` → current UTC timestamp
4. Phase 1 `session_started_at` → `null`
5. `current_phase` → `2`

Update Phase 1 task to `completed`.

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.8.1 — Phase 1: Start — COMPLETE (<formatted_time>)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Research?"
>
> - **Yes, start Phase 2 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 2 now" and "Not yet"

**If Yes** — invoke the `flow:research` skill using the Skill tool.

**If Not yet** — print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```
````

Then report:
- Worktree location
- PR link
- Whether baseline `bin/ci` was clean
- Which gems were upgraded (`git diff Gemfile.lock` summary)
- Confirmation `bin/ci` is green
