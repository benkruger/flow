---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades gems, opens a PR, creates .claude/sdlc-states/<branch>.json, and configures the workspace. Usage: /sdlc:start <feature name words>"
---

# SDLC Start — Phase 1: Start

## Usage

```
/sdlc:start app payment webhooks
```

Arguments become the feature name. Words are joined with hyphens:
- Branch: `app-payment-webhooks`
- Worktree: `.worktrees/app-payment-webhooks`
- PR title: `App Payment Webhooks`

<HARD-GATE>
Do NOT proceed if the feature name is missing. Ask the user:
"What is the feature name? e.g. /sdlc:start app payment webhooks"
</HARD-GATE>

## Announce

Print:

```
============================================
  SDLC — Phase 1: Start — STARTING
============================================
```

## Steps

### Step 1 — Check for existing feature

Run this check first:

```bash
python3 << 'PYCHECK'
import json, sys
from pathlib import Path

state_dir = Path(".claude/sdlc-states")
if state_dir.exists():
    files = list(state_dir.glob("*.json"))
    if files:
        names = [f.stem for f in files]
        print(f"WARNING: Active SDLC feature(s) found: {', '.join(names)}")
        sys.exit(1)
sys.exit(0)
PYCHECK
```

If this exits non-zero, use AskUserQuestion:

> "An active SDLC feature already exists. What would you like to do?"
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

### Step 2 — Pull main

```bash
git pull origin main
```

If this fails, stop and report why.

### Step 3 — Create the worktree

```bash
git worktree add .worktrees/<feature-name> -b <feature-name>
```

All subsequent commands that touch application code run inside the worktree.

### Step 4 — Push branch to remote immediately

```bash
git push -u origin <feature-name>
```

### Step 5 — Open the PR

```bash
gh pr create \
  --title "<Feature Name Title Cased>" \
  --body "## What\n\n<Feature name as a sentence.>" \
  --base main
```

Capture the PR URL from the output. Extract the PR number from the URL.

### Step 6 — Create the SDLC state file

Create `.claude/sdlc-states/` directory if it does not exist. Write the state
file at `.claude/sdlc-states/<branch-name>.json` with the current UTC timestamp:

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
    "1":  { "name": "Start",     "status": "in_progress", "started_at": "<now>", "completed_at": null, "session_started_at": "<now>", "cumulative_seconds": 0, "visit_count": 1 },
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
- Phase 1 (Start): `in_progress`
- Phases 2–10: `pending`

### Step 7 — Configure workspace permissions

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

**If it exists**, read it and merge in any missing entries. Do not remove existing entries. No duplicates.

### Step 8 — Baseline `bin/ci`

```bash
cd .worktrees/<feature-name> && bin/ci
```

- **Passes** — note as baseline and continue
- **Fails** — report failures clearly (pre-existing issues). Ask user whether to proceed or stop.

### Step 9 — Upgrade gems

```bash
cd .worktrees/<feature-name> && bundle update
```

### Step 10 — Post-update `bin/ci`

```bash
cd .worktrees/<feature-name> && bin/ci
```

- **Passes** — continue to Step 12
- **Fails** — continue to Step 11

### Step 11 — Fix breakage from gem upgrade

**RuboCop violations:**
```bash
cd .worktrees/<feature-name> && rubocop -A && bin/ci
```

**Test failures** — read output carefully, fix call sites or fixtures, repeat until green.

<HARD-GATE>
Do NOT proceed to Step 12 until bin/ci is green. If not fixed after
three attempts, stop and report exactly what is failing and what was tried.
</HARD-GATE>

### Step 12 — Commit and push

Use `/sdlc:commit` to review and commit the changes (`Gemfile.lock` + any gem fixes).

### Done — Update state and complete phase

Update `.claude/sdlc-states/<branch>.json`:
1. `cumulative_seconds` for Phase 1: `current_time - session_started_at`
2. Phase 1 `status` → `complete`
3. Phase 1 `completed_at` → current UTC timestamp
4. Phase 1 `session_started_at` → `null`
5. `current_phase` → `2`

Update Phase 1 task to `completed`.

Use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Research?"
> - **Yes, start Phase 2 now**
> - **Not yet**

**If Yes** — invoke the `sdlc:research` skill using the Skill tool. Also print:

```
============================================
  SDLC — Phase 1: Start — COMPLETE
============================================
```

**If Not yet** — print:

```
============================================
  SDLC — Paused
  Run /sdlc:resume when ready to continue.
============================================
```

Then report:
- Worktree location
- PR link
- Whether baseline `bin/ci` was clean
- Which gems were upgraded (`git diff Gemfile.lock` summary)
- Confirmation `bin/ci` is green
