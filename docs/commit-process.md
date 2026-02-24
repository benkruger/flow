# Commit Process

Shared process used by both `/commit` (maintainer) and `/flow:commit` (FLOW plugin).
Each calling skill handles its own announce and logging, then follows these steps.

## Step 0 — Run tests

Run `bin/ci`. If any test fails, stop and report the failure.
Do not proceed to diff review until tests pass.

## Step 1 — Show the diff

First run `git status` to see what changed. If nothing to commit, tell the user and stop.

Then stage everything and diff the staged changes:

```bash
git add -A
```

```bash
git diff --cached
```

This ensures new (untracked) files appear in the diff output — `git diff HEAD`
misses untracked files entirely. Staging first gives one unified diff with
consistent formatting for all changes.

Render the output directly in your response — do not ask the user to expand tool output.

If the diff is too large to render inline (the Bash tool truncates and
persists the output), use `git diff --cached --stat` for the summary
and read the persisted output file with the Read tool. Never redirect
output to `/tmp/` — shell redirects trigger permission prompts.

**Format the status as:**
```
**Status**
modified:   path/to/file.rb
new file:   path/to/other.rb
deleted:    path/to/removed.rb
```

**Format the diff as a fenced diff code block:**
````
```diff
- removed line
+ added line
```
````

The `diff` code block renders red/green in most markdown environments.

## Step 2 — Commit Message

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

**Before displaying your draft, verify it contains all of these in order:**

1. Subject line — imperative verb, ≤72 chars, no period
2. Blank line
3. The literal word `tl;dr` on its own line — no colon, no elaboration, just `tl;dr`
4. Blank line
5. Explanation paragraph — the WHY, not the what
6. Blank line
7. File list — one bullet per changed file with reason

If any element is missing or out of order, rewrite before displaying.

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

## Step 3 — Ask for approval

Use the `AskUserQuestion` tool with exactly these two options:

Question: "Approve this commit?"
- Option 1: **Approve** — "Looks good, commit and push"
- Option 2: **Deny** — "Something needs to be fixed first"

## Step 4 — Commit and push (on approval)

Files are already staged from Step 1. No need to `git add -A` again.

1. Use the Write tool to write the commit message to `/tmp/flow-commit-<repo>-<branch>.txt` (where `<repo>` is the repository directory name and `<branch>` is from `git branch --show-current`), then commit and delete the temp file:
   ```
   git commit -F /tmp/flow-commit-<repo>-<branch>.txt && rm /tmp/flow-commit-<repo>-<branch>.txt
   ```
   - Repo+branch scoped filename prevents collisions between concurrent sessions across different repos
   - The `rm` prevents the Write tool from showing a confusing diff of old→new message on the next commit
   - The Write tool handles newlines and special characters safely — no shell escaping needed
   - Never use `python3 -c` to write the message — literal `$(...)` in the body triggers command substitution warnings
   - Never use `git commit -m` with heredoc — the multi-line command fails permission pattern matching
3. `git pull origin <current-branch>` — pull before pushing to pick up any changes merged while you were working
4. If the pull produced merge conflicts:
   - Run `git status` to identify every conflicting file
   - Read each conflicting file carefully — understand both sides:
     - `<<<<<<<` (HEAD) = our changes
     - `>>>>>>>` (incoming) = what was merged to main
   - For each conflict, attempt to resolve it intelligently:
     - If both sides add different things that don't logically conflict → keep both
     - If one side removes something the other side modified → understand intent, apply the right resolution
     - If the resolution is obvious from context → fix it silently, `git add <file>`
   - Only escalate to the user if a conflict requires a domain or business decision you cannot make — show exactly that conflict and ask specifically what to do
   - Once all conflicts are resolved: `git add -A`, then continue to push
5. If pull was clean: `git push`
6. Confirm success and show the commit SHA.

## Step 5 — Handle denial

Unstage everything first (files were staged in Step 1 for diff purposes):

```bash
git reset HEAD
```

`git reset HEAD` only unstages — it moves files back from staged to unstaged.
No code is deleted, no changes are lost. It is the opposite of `git add`.

Then ask: **What needs to be addressed before committing?**

Listen to the reason, acknowledge it clearly, and stop. Do not commit. The user
will make fixes and re-invoke the commit skill when ready.

## Hard Rules

- Never commit without showing the diff first
- Never skip the approval step
- Never use `--no-verify`
- Never add Co-Authored-By trailers or attribution lines — commits are authored by the user alone
- Always pull before pushing — other sessions may have merged changes
- **Never rebase — ever.** Always merge. `git rebase` is forbidden.