---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades gems, opens a PR, creates .claude/ror-state.json, and configures the workspace. Usage: /ror:start <feature name words>"
---

# ROR Start — Phase 1: Start

## Usage

```
/ror:start app payment webhooks
```

Arguments become the feature name. Words are joined with hyphens:
- Branch: `app-payment-webhooks`
- Worktree: `.worktrees/app-payment-webhooks`
- PR title: `App Payment Webhooks`

<HARD-GATE>
Do NOT proceed past Step 1 if the feature name is missing. Ask the user: "What is the feature name? e.g. /ror:start app payment webhooks"
</HARD-GATE>

## Announce

At the very start, before doing anything, print:

```
============================================
  ROR — Phase 1: Start — STARTING
============================================
```

## Steps

### Step 1 — Pull main

```bash
git pull origin main
```

Ensure the starting point is current. If this fails, stop and report why.

### Step 2 — Create the worktree

```bash
git worktree add .worktrees/<feature-name> -b <feature-name>
```

Example: `git worktree add .worktrees/app-payment-webhooks -b app-payment-webhooks`

All subsequent steps run inside the worktree directory.

### Step 3 — Push branch to remote immediately

```bash
git push -u origin <feature-name>
```

Establishes the branch remotely before any code changes.

### Step 4 — Open the PR

```bash
gh pr create \
  --title "<Feature Name Title Cased>" \
  --body "## What\n\n<Feature name as a sentence.>" \
  --base main
```

Capture the PR URL from the output. Extract the PR number from the URL (the trailing integer).

### Step 5 — Create the ROR state file

Create `.claude/ror-state.json` at the project root (not inside the worktree). Use the current UTC timestamp for `started_at` and `session_started_at` on Phase 1.

```json
{
  "feature": "<Feature Name Title Cased>",
  "branch": "<feature-name>",
  "worktree": ".worktrees/<feature-name>",
  "pr_number": <pr_number>,
  "pr_url": "<pr_url>",
  "started_at": "<current_utc_timestamp>",
  "current_phase": 1,
  "phases": {
    "1":  { "name": "Start",     "status": "in_progress", "started_at": "<current_utc_timestamp>", "completed_at": null, "session_started_at": "<current_utc_timestamp>", "cumulative_seconds": 0, "visit_count": 1 },
    "2":  { "name": "Research",  "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "3":  { "name": "Design",    "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "4":  { "name": "Plan",      "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "5":  { "name": "Implement", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "6":  { "name": "Test",      "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "7":  { "name": "Review",    "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "8":  { "name": "Ship",      "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "9":  { "name": "Reflect",   "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
    "10": { "name": "Cleanup",   "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 }
  }
}
```

Then create a task for each phase using TaskCreate:
- Phase 1 (Start): status `in_progress`
- Phases 2–10: status `pending`

### Step 6 — Configure workspace permissions

Check if `.claude/settings.json` exists in the project root.

**If it does not exist**, create it:

```json
{
  "permissions": {
    "allow": [
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push -u *)",
      "Bash(git worktree *)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(python3 *)"
    ]
  }
}
```

**If it exists**, read it and merge in any missing entries. Do not remove or overwrite existing entries. Do not add duplicates.

### Step 7 — Baseline `bin/ci`

Run `bin/ci` inside the worktree. This captures the health of the codebase before any changes.

- If it **passes** — note it as the baseline and continue.
- If it **fails** — report the failures clearly. These are pre-existing issues, not caused by your changes. Ask the user whether to proceed anyway or stop.

### Step 8 — Upgrade gems

```bash
bundle update
```

Upgrades all gems to their latest compatible versions inside the worktree.

### Step 9 — Post-update `bin/ci`

Run `bin/ci` again after the gem upgrade.

- If it **passes** — continue to Step 11.
- If it **fails** — continue to Step 10.

### Step 10 — Fix breakage from gem upgrade

**RuboCop violations** — run the auto-fixer first:
```bash
rubocop -A
```
Then run `bin/ci` again. If violations remain that cannot be auto-fixed, read the output and fix them manually one by one.

**Test failures** — read the failure output carefully. These are typically caused by:
- Changed gem APIs (update the call sites)
- New validation behaviour (update test fixtures or assertions)
- Deprecation warnings promoted to errors (follow the deprecation message)

Fix each failure, then run `bin/ci` again. Repeat until green.

<HARD-GATE>
Do NOT proceed to Step 11 until bin/ci is green. If you cannot fix the failures after three attempts, stop and report exactly what is failing and what you tried.
</HARD-GATE>

### Step 11 — Commit and push

Use `/ror:commit` to review and commit the changes (`Gemfile.lock` and any gem-related fixes).

### Done — Update state and complete phase

Update `.claude/ror-state.json`:
1. Calculate `cumulative_seconds` for Phase 1: `current_time - session_started_at`
2. Set Phase 1 `status` to `complete`
3. Set Phase 1 `completed_at` to current UTC timestamp
4. Set Phase 1 `session_started_at` to `null`
5. Set `current_phase` to `2`

Update the Phase 1 task to `completed`.

Ask the user:

> "Phase 1: Start is complete. Ready to proceed to Phase 2: Research?"
> - **Yes, proceed** — print the completion banner
> - **No, stay here** — ask what still needs to be done

On approval, print:

```
============================================
  ROR — Phase 1: Start — COMPLETE
  Next: Phase 2: Research  (/ror:research)
============================================
```

Then report a summary:
- Branch and worktree location
- PR link
- Whether baseline `bin/ci` was clean or had pre-existing issues
- Which gems were upgraded (run `git diff Gemfile.lock` to summarise)
- Confirmation that `bin/ci` is green
