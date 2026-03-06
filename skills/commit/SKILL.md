---
name: commit
description: "Review the full diff, approve or deny, then git add + commit + push. Use at every commit checkpoint in the FLOW workflow."
model: sonnet
---

# Commit

Review all pending changes as a diff before committing. You must get explicit approval before touching git.

## Mode Detection

Determine the operating mode before proceeding:

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Read `<project_root>/.flow-states/<branch>.json` with the Read tool.
   - **File exists** (content returned) → **FLOW** mode
   - **File does not exist** (error returned) → use Glob to check for `flow-phases.json` in the project root.
     - Exists → **Maintainer** mode (this is the plugin source repo)
     - Does not exist → **Standalone** mode

Keep the project root, branch, and detected mode in context for the rest of this skill.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

**FLOW mode:**

````text
```
============================================
  FLOW v0.16.2 — flow:commit — STARTING
============================================
```
````

**Maintainer and Standalone mode:**

````text
```
============================================
  Commit — STARTING
============================================
```
````

On completion (whether approved or denied), print the same way:

**FLOW mode:**

````text
```
============================================
  FLOW v0.16.2 — flow:commit — COMPLETE
============================================
```
````

**Maintainer and Standalone mode:**

````text
```
============================================
  Commit — COMPLETE
============================================
```
````

## Flag: --auto

When the user invokes `/flow:commit --auto`, skip the Step 3 approval prompt and proceed directly to Step 4 (commit and push). Everything else is identical: `bin/flow ci`, diff display, commit message generation and display, pull-before-push.

In FLOW mode, Python projects also skip approval — see Step 3.

`--auto` is user-invoked only. Claude must never call `/flow:commit --auto` programmatically — except in `/flow:reflect`, which is fully autonomous and commits without mid-process approval.

---

## Process

### Step 0 — Run tests

Run `bin/flow ci --if-dirty`. This skips the run if no files changed since the
last green run. If any test fails, stop and report the failure.
Do not proceed to diff review until tests pass.

### Step 1 — Show the diff

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

```text
**Status**
modified:   path/to/file.rb
new file:   path/to/other.rb
deleted:    path/to/removed.rb
```

**Format the diff as a fenced diff code block:**

````text
```diff
- removed line
+ added line
```
````

The `diff` code block renders red/green in most markdown environments.

#### Docs sync check

**FLOW and Maintainer mode only.** Skip for Standalone.

If the diff includes changes to any of these files:

- `skills/*/SKILL.md` — check `docs/skills/` and `docs/phases/` for matching updates
- `flow-phases.json` — check `docs/phases/`, `docs/skills/index.md`, `README.md`, `docs/index.html`
- `docs/reference/flow-state-schema.md` — check against `conftest.make_state()` fields

Flag any docs that may need updates before writing the commit message. If docs are already current, proceed.

### Step 2 — Commit Message

Write a commit message that a developer reading `git log` six months from now would find genuinely useful.

**Structure:**

```text
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
- Describe the goal, not the mechanism — when a change has both, the subject says why it matters. "Consolidate 7 permission entries into 1" (goal) not "Move scripts from hooks/ to lib/" (mechanism)
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

If `--auto` was passed, skip this step and proceed directly to Step 4.

**FLOW mode only:** If the project framework is `python` (read `.flow.json`), also skip this step.

Otherwise, use the `AskUserQuestion` tool with exactly these two options:

Question: "Approve this commit?"
- Option 1: **Approve** — "Looks good, commit and push"
- Option 2: **Deny** — "Something needs to be fixed first"

### Step 4 — Commit and push (on approval)

Files are already staged from Step 1. No need to `git add -A` again.

1. Use the Write tool to write the commit message to `.flow-commit-msg` in the project root.
   - Each worktree has its own project root, so concurrent sessions don't collide
   - The file is inside the project, so the Write tool has permission without prompting
   - The Write tool handles newlines and special characters safely — no shell escaping needed
   - Never write to `/tmp/` — paths outside the project trigger permission prompts that settings.json cannot suppress
   - Never use `python3 -c` to write the message — literal `$(...)` in the body triggers command substitution warnings
   - Never use `git commit -m` with heredoc — the multi-line command fails permission pattern matching
2. Commit from the temp file:

   ```bash
   git commit -F .flow-commit-msg
   ```

3. Delete the temp file:

   ```bash
   rm .flow-commit-msg
   ```

   The `rm` prevents the Write tool from showing a confusing diff of old→new message on the next commit.
4. `git pull origin <current-branch>` — pull before pushing to pick up any changes merged while you were working
5. If the pull produced merge conflicts:
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
6. If pull was clean: `git push`
7. Confirm success and show the commit SHA.

### Step 5 — Handle denial

Unstage everything first (files were staged in Step 1 for diff purposes):

```bash
git reset HEAD
```

`git reset HEAD` only unstages — it moves files back from staged to unstaged.
No code is deleted, no changes are lost. It is the opposite of `git add`.

Then ask: **What needs to be addressed before committing?**

Listen to the reason, acknowledge it clearly, and stop. Do not commit. The user
will make fixes and re-invoke the commit skill when ready.

### Hard Rules

- Never commit without showing the diff first
- Never skip the approval step — unless `--auto` was passed by the user or the project framework is `python` (FLOW mode only)
- `--auto` is user-invoked only. Claude must never call `/flow:commit --auto` programmatically — except in `/flow:reflect`, which is fully autonomous and commits without mid-process approval.
- Never use `--no-verify`
- Never add Co-Authored-By trailers or attribution lines — commits are authored by the user alone
- Always pull before pushing — other sessions may have merged changes
- **Never rebase — ever.** Always merge. `git rebase` is forbidden.

## Additional Rules

- **FLOW mode only:** If `bin/flow ci` has not been run since the last code change, warn the user before asking for approval
