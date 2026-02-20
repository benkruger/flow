---
name: commit
description: "Review the full diff, approve or deny, then git add + commit + push. Use at every commit checkpoint in the ROR workflow."
---

# ROR Commit

Review all pending changes as a diff before committing. You must get explicit approval before touching git.

## Process

### Step 1 — Show the diff

Run these as two separate commands:

1. `git status` — show what files have changed
2. `git diff HEAD` — show the full diff

Display the diff output in a `diff` code block so the user can review red/green inline.

If `git status` shows nothing to commit, tell the user and stop.

### Step 2 — Commit Message

Write a commit message that a developer reading `git log` six months from now would find genuinely useful.

**Structure:**
```
Short subject line (imperative verb, under 72 characters)

tl;dr

One or two sentences explaining the WHY — what problem this solves,
what behaviour changes, or what was wrong before.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
- path/to/another.rb: What changed and why
```

Note: `tl;dr` is on its own line with a blank line before the paragraph.

**Subject line rules:**
- Start with an imperative verb: Add, Fix, Update, Remove, Refactor, Extract
- No prefix jargon (no `feat:`, `chore:`, `fix:` — just the verb)
- Under 72 characters
- No period at the end

**Body rules:**
- Blank line between subject and body
- Explain the motivation — what prompted this change?
- List each meaningful change with its file and a plain-English reason
- Call out explicitly if the diff includes migrations, schema changes, or Gemfile changes
- Do not pad with obvious restatements of the diff

Display the full message under the heading **Commit Message** before asking for approval.

### Step 3 — Ask for approval

Use the `AskUserQuestion` tool with exactly these two options:

Question: "Approve this commit?"
- Option 1: **Approve** — "Looks good, commit and push"
- Option 2: **Deny** — "Something needs to be fixed first"

### Step 4 — Commit and push (on approval)

1. `git add -A`
2. `git commit -m "<message from Step 2>"`
3. `git push`
4. Confirm success and show the commit SHA.

### Step 5 — Handle denial

Ask: **What needs to be addressed before committing?**

Listen to the reason, acknowledge it clearly, and stop. Do not commit. The user will make fixes and invoke `/ror:commit` again when ready.

## Rules

- Never commit without showing the diff first
- Never skip the approval step
- Never use `--no-verify`
- If `bin/ci` has not been run since the last code change, warn the user before asking for approval
