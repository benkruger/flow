---
name: start
description: "Phase 0: Prepare — begin a new feature. Creates a worktree, upgrades gems, opens a PR, and configures the workspace. Usage: /ror:start <feature name words>"
---

# ROR Start — Phase 0: Prepare

## Usage

```
/ror:start app payment webhooks
```

Arguments become the feature name. Words are joined with hyphens:
- Branch: `app-payment-webhooks`
- Worktree: `.worktrees/app-payment-webhooks`
- PR title: `App payment webhooks`

<HARD-GATE>
Do NOT proceed past Step 1 if the feature name is missing. Ask the user: "What is the feature name? e.g. /ror:start app payment webhooks"
</HARD-GATE>

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
  --body "## What\n\n<Feature name as a sentence.>\n\n## Status\n\n- [ ] Phase 0: Prepare\n- [ ] Phase 1: Research\n- [ ] Phase 2: Design\n- [ ] Phase 3: Plan\n- [ ] Phase 4: Implement\n- [ ] Phase 5: Test\n- [ ] Phase 6: Review\n- [ ] Phase 7: Ship" \
  --base main
```

The PR body is auto-generated from the feature name. The phase checklist tracks progress.

### Step 5 — Configure workspace permissions

Check if `.claude/settings.json` exists in the project root.

**If it does not exist**, create it:

```json
{
  "permissions": {
    "allow": [
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push -u *)"
    ]
  }
}
```

**If it exists**, read it and merge in any missing entries from the allow list above. Do not remove or overwrite existing entries. Do not add duplicates.

### Step 6 — Baseline `bin/ci`

Run `bin/ci` inside the worktree. This captures the health of the codebase before any changes.

- If it **passes** — note it as the baseline and continue.
- If it **fails** — report the failures clearly. These are pre-existing issues, not caused by your changes. Ask the user whether to proceed anyway or stop.

### Step 7 — Upgrade gems

```bash
bundle update
```

Upgrades all gems to their latest compatible versions inside the worktree.

### Step 8 — Post-update `bin/ci`

Run `bin/ci` again after the gem upgrade.

- If it **passes** — continue to Step 10.
- If it **fails** — continue to Step 9.

### Step 9 — Fix breakage from gem upgrade

Gem updates commonly cause two types of failures:

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
Do NOT proceed to Step 10 until bin/ci is green. If you cannot fix the failures after three attempts, stop and report exactly what is failing and what you tried.
</HARD-GATE>

### Step 10 — Commit and push

Use `/ror:commit` to review and commit the changes (Gemfile.lock + any gem-related fixes).

The commit message should be: `chore: bundle update`

### Done

Report a summary:
- Branch and worktree created
- PR link
- Whether baseline `bin/ci` was clean or had pre-existing issues
- Which gems were upgraded (run `git diff Gemfile.lock` to summarise)
- Confirmation that `bin/ci` is green
