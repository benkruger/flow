---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades gems, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:start <feature name words>"
---

# FLOW Start — Phase 1: Start

## Usage

```
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

````
```
============================================
  FLOW v0.6.3 — Phase 1: Start — STARTING
  Recommended model: Haiku
============================================
```
````

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command with exit code capture:

```bash
COMMAND; EC=$?; exit $EC
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```
YYYY-MM-DDTHH:MM:SSZ [Phase 1] Step X — desc (exit EC)
```

Use the feature name as `<branch>` — it matches the branch name.

Begin logging at Step 8. Steps 2–7 are not logged (state directory not yet created).

---

## Steps

### Step 1 — Check for existing feature

Use the Glob tool to check for existing state files matching `.flow-states/*.json`.

If any files are found, list their names (the branch names from the filenames).

If any files are found, use AskUserQuestion:

> "An active FLOW feature already exists. What would you like to do?"
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

### Step 2 — Pull main

```bash
git pull origin main
```

If this fails, stop and report why.

### Step 3 — Create the worktree and cd into it

```bash
git worktree add .worktrees/<feature-name> -b <feature-name>
```

Then change into the worktree:

```bash
cd .worktrees/<feature-name>
```

The Bash tool persists working directory between calls, so all subsequent
commands run inside the worktree automatically. Do NOT repeat `cd .worktrees/`
in later steps — it would look for a nested `.worktrees/` that doesn't exist.

### Step 4 — Configure git exclude

Ensure `.flow-states/` and `.worktrees/` are excluded from version control
using the per-repo local exclude (not committed, not in `.gitignore`).

```bash
git rev-parse --git-common-dir
```

Read `<git-common-dir>/info/exclude`. If `.flow-states/` or `.worktrees/`
are missing from the file, append them. Use the Edit tool (or Write if the
file does not exist) — one entry per line.

### Step 5 — Configure workspace permissions

Check if `.claude/settings.json` exists in the **project root** (one level up from the worktree).

**If it does not exist**, create it:

```json
{
  "permissions": {
    "allow": [
      "Bash(cd .worktrees/* && *)",
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push;*)",
      "Bash(git push -u *)",
      "Bash(git reset HEAD)",
      "Bash(git reset HEAD;*)",
      "Bash(git worktree *)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(git push origin --delete *)",
      "Bash(python3 *)",
      "Bash(bin/ci)",
      "Bash(bin/ci;*)",
      "Bash(bin/rails test *)",
      "Bash(rubocop *)",
      "Bash(rubocop -A)",
      "Bash(bundle update)",
      "Bash(bundle update;*)",
      "Bash(rm .flow-commit-*)",
      "Bash(bundle exec *)"
    ]
  }
}
```

**If it exists**, read it and merge in any missing entries. Do not remove existing entries. No duplicates.

### Step 6 — Initial commit, push, and open PR

GitHub requires at least one commit between base and head to create a PR.
Already inside the worktree from Step 3:

```bash
git commit --allow-empty -m "Start <feature-name> branch"
```

```bash
git push -u origin <feature-name>
```

```bash
gh pr create \
  --title "<Feature Name Title Cased>" \
  --body "## What\n\n<Feature name as a sentence.>" \
  --base main
```

Capture the PR URL from the output. Extract the PR number from the URL.

### Step 7 — Create the FLOW state file

Use the Write tool to write the state file at `.flow-states/<branch-name>.json`
with the current UTC timestamp. The Write tool creates parent directories automatically.

```json
{
  "feature": "<Feature Name Title Cased>",
  "branch": "<feature-name>",
  "worktree": ".worktrees/<feature-name>",
  "pr_number": <pr_number>,
  "pr_url": "<pr_url>",
  "started_at": "<current_utc_timestamp>",
  "current_phase": 1,
  "notes": [],
  "phases": {
    "1":  { "name": "Start",     "status": "in_progress", "started_at": "<now>", "completed_at": null, "session_started_at": "<now>", "cumulative_seconds": 0, "visit_count": 1 },
    "2":  { "name": "Research",  "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "3":  { "name": "Design",    "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "4":  { "name": "Plan",      "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "5":  { "name": "Code", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "6":  { "name": "Review",   "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "7":  { "name": "Reflect",   "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "8":  { "name": "Cleanup",   "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 }
  }
}
```

### Step 8 — Baseline `bin/ci`

```bash
bin/ci
```

- **Passes** — note as baseline and continue
- **Fails** — launch the CI fix sub-agent (see Step 11). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — use `/flow:commit` to commit the fix, then continue
  - **Not fixed** — stop and report to the user what is failing

### Step 9 — Upgrade gems

```bash
bundle update
```

### Step 10 — Post-update `bin/ci`

```bash
bin/ci
```

- **Passes** — continue to Step 12
- **Fails** — launch the CI fix sub-agent (see Step 11). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — continue to Step 12 (Gemfile.lock + fixes committed together)
  - **Not fixed** — stop and report to the user what is failing

### Step 11 — CI fix sub-agent

When `bin/ci` fails in Step 8 or Step 10, launch a sub-agent to diagnose
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
> - Status: fixed / not_fixed
> - What was wrong
> - What was changed (files modified)

Wait for the sub-agent to return.

<HARD-GATE>
Do NOT proceed past Step 8 or Step 10 until bin/ci is green.
</HARD-GATE>

### Step 12 — Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` + any gem fixes).

### Done — Update state and complete phase

Update `.flow-states/<branch>.json`:
1. `cumulative_seconds` for Phase 1: `current_time - session_started_at`
2. Phase 1 `status` → `complete`
3. Phase 1 `completed_at` → current UTC timestamp
4. Phase 1 `session_started_at` → `null`
5. `current_phase` → `2`

Update Phase 1 task to `completed`.

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Research?"
> - **Yes, start Phase 2 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 2 now" and "Not yet"

**If Yes** — invoke the `flow:research` skill using the Skill tool. Also print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — Phase 1: Start — COMPLETE
============================================
```
````

**If Not yet** — print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
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