---
name: commit
description: "Review the full diff, approve or deny, then git add + commit + push. Use at every commit checkpoint in the ROR workflow."
---

# ROR Commit

Review all pending changes as a diff before committing. You must get explicit approval before touching git.

## Process

### Step 1 — Show the diff

Run `git diff HEAD` to capture all changes (staged and unstaged). Display the full output in a `diff` code block so the user can review red/green inline.

If there is nothing to commit (`git status` shows clean), tell the user and stop.

### Step 2 — Summarize what changed

Below the diff, write a brief plain-English summary:
- Which files changed and why
- Any migrations, schema changes, or Gemfile changes (call these out explicitly)
- Anything that looks risky or unexpected

### Step 3 — Ask for approval

Use the `AskUserQuestion` tool with exactly these two options:

Question: "Approve this commit?"
- Option 1: **Approve** — "Looks good, commit and push"
- Option 2: **Deny** — "Something needs to be fixed first"

### Step 4 — Commit and push (on approval)

1. `git add -A`
2. Generate a commit message using conventional commits format:
   - `feat:` new feature or behaviour
   - `fix:` bug fix
   - `chore:` dependencies, tooling, config (e.g. `bundle update`)
   - `refactor:` restructuring without behaviour change
   - `test:` test changes only
   - Keep the subject line under 72 characters
   - No body unless something non-obvious needs explanation
3. `git commit -m "<generated message>"`
4. `git push`
5. Confirm success and show the commit SHA.

### Step 5 — Handle denial

Ask: **What needs to be addressed before committing?**

Listen to the reason, acknowledge it clearly, and stop. Do not commit. The user will make fixes and invoke `/ror:commit` again when ready.

## Rules

- Never commit without showing the diff first
- Never skip the approval step
- Never use `--no-verify`
- If `bin/ci` has not been run since the last code change, warn the user before asking for approval
