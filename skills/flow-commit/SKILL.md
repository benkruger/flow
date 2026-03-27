---
name: flow-commit
description: "Review the full diff, approve or deny, then git add + commit + push. Use at every commit checkpoint in the FLOW workflow."
---

# Commit

Review all pending changes as a diff before committing. You must get explicit approval before touching git.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Round 1 — Setup

Run all of these in parallel (one response, multiple tool calls):

1. `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
2. `git branch --show-current` — this is the current branch.

Keep the project root and branch in context for the rest of this skill.

## Round 2 — Mode and Format Detection

Run all of these in parallel (one response, all use the project root from Round 1):

1. Use the Glob tool: pattern `*.json`, path `<project_root>/.flow-states` — if any results, this is **FLOW** mode.
2. Use the Glob tool: pattern `flow-phases.json`, path `<project_root>` — if found (and no state files from #1), this is **Maintainer** mode.
3. Use the Read tool: read `<project_root>/.flow.json`.
   - Parse the `commit_format` value: `"title-only"` or `"full"`.
   - If `.flow.json` does not exist or has no `commit_format` key → use `"full"`.

If neither Glob returned results → **Standalone** mode.

Keep the detected mode and `commit_format` in context.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

**FLOW mode:**

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — flow:flow-commit — STARTING
──────────────────────────────────────────────────
```
````

**Maintainer and Standalone mode:**

````markdown
```text
──────────────────────────────────────────────────
  Commit — STARTING
──────────────────────────────────────────────────
```
````

On completion (whether approved, denied, or nothing to commit), print the same way:

**FLOW mode:**

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — flow:flow-commit — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

**Maintainer and Standalone mode:**

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ Commit — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Usage

```text
/flow:flow-commit
/flow:flow-commit --auto
/flow:flow-commit --manual
```

- `/flow:flow-commit` — defaults to auto (no approval prompt)
- `/flow:flow-commit --auto` — skips the approval prompt
- `/flow:flow-commit --manual` — requires explicit approval

## Mode Resolution

1. If `--auto` was passed → mode is **auto**
2. If `--manual` was passed → mode is **manual**
3. Otherwise → mode is **auto**

`--auto` is user-invoked only. Claude must never call `/flow:flow-commit --auto` programmatically — except in phase skills (`/flow:flow-start`, `/flow:flow-code`, `/flow:flow-code-review`, `/flow:flow-learn`) which commit autonomously as part of their workflow.

---

## Process

### Round 3 — Test and stage

**FLOW and Maintainer mode:** run both in parallel (one response, two Bash calls):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

```bash
git add -A
```

CI and staging are independent — CI tests the working tree, staging indexes it. If CI
fails, stop and report the failure. Staged files are harmless if CI fails — nothing commits.

**Standalone mode:** run only `git add -A` (skip CI).

### Round 4 — Show the diff

Run both in parallel (one response, two Bash calls):

```bash
git status
```

```bash
git diff --cached
```

If `git diff --cached` is empty, tell the user "Nothing to commit", print the COMPLETE banner, and return to the caller.

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

````markdown
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

### Step 1 — Commit Message

Write a commit message that a developer reading `git log` six months from now would find genuinely useful.

Use the `commit_format` from Round 2 to determine the structure.

**If `commit_format` is `"full"`:**

```text
Full-sentence subject line (imperative verb + what + why, ends with a period.)

tl;dr

One or two sentences explaining the WHY — what problem this solves,
what behaviour changes, or what was wrong before.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
- path/to/another.rb: What changed and why
```

Before displaying your draft, verify it contains all of these in order:

1. Subject line — imperative verb, what + why in one sentence, ends with a period
2. Blank line
3. The literal word `tl;dr` on its own line — no colon, no elaboration, just `tl;dr`
4. Blank line
5. Explanation paragraph — the WHY, not the what
6. Blank line
7. File list — one bullet per changed file with reason

If any element is missing or out of order, rewrite before displaying.

**If `commit_format` is `"title-only"`:**

```text
Full-sentence subject line (imperative verb + what + why, ends with a period.)

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
- path/to/another.rb: What changed and why
```

Before displaying your draft, verify it contains all of these in order:

1. Subject line — imperative verb, what + why in one sentence, ends with a period
2. Blank line
3. File list — one bullet per changed file with reason

If any element is missing or out of order, rewrite before displaying.

**Subject line rules (both formats):**
- Start with an imperative verb: Add, Fix, Update, Remove, Refactor, Extract
- Include the business reason — why this change matters, not just what changed. "Remove /flow-qa skill and dev-mode plumbing because Claude Code's --plugin-dir flag makes QA testing trivial."
- Describe the goal, not the mechanism — when a change has both, the subject says why it matters
- No prefix jargon (no `feat:`, `chore:`, `fix:` — just the verb)
- Ends with a period (it is a full sentence)

**Body rules (both formats):**
- Blank line between subject and body
- List each meaningful change with its file and a plain-English reason
- Call out explicitly if the diff includes migrations, schema changes, or Gemfile changes
- Do not pad with obvious restatements of the diff

**Additional body rules (full format only):**
- Explain the motivation — what prompted this change?

Display the full message under the heading **Commit Message** before asking for approval.

### Step 2 — Ask for approval

**Unless `--manual` was explicitly passed, skip this step entirely — the default is auto.**

If `--manual` was explicitly passed, use the `AskUserQuestion` tool with exactly these two options:

Question: "Approve this commit?"
- Option 1: **Approve** — "Looks good, commit and push"
- Option 2: **Deny** — "Something needs to be fixed first"

### Round 5 — Commit and push (on approval)

Files are already staged from Round 3. No need to `git add -A` again.

1. Use the Write tool to write the commit message to `.flow-commit-msg` in the project root.
   - Each worktree has its own project root, so concurrent sessions don't collide
   - The file is inside the project, so the Write tool has permission without prompting
   - The Write tool handles newlines and special characters safely — no shell escaping needed
   - Never write to `/tmp/` — paths outside the project trigger permission prompts that settings.json cannot suppress
   - Never use `python3 -c` to write the message — literal `$(...)` in the body triggers command substitution warnings
   - Never use `git commit -m` with heredoc — the multi-line command fails permission pattern matching

### Round 6 — Finalize

Run the finalize script to commit, clean up the message file, pull, and push in one call:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow finalize-commit .flow-commit-msg <current-branch>
```

The script returns JSON:

- `{"status": "ok", "sha": "..."}` — success. Confirm and show the commit SHA.
- `{"status": "conflict", "files": [...]}` — merge conflicts from pull. Resolve each conflicting file:
  - Read each conflicting file carefully — understand both sides
  - If both sides add different things that don't logically conflict → keep both
  - If one side removes something the other side modified → understand intent, apply the right resolution
  - If the resolution is obvious from context → fix it silently, `git add <file>`
  - Only escalate to the user if a conflict requires a domain or business decision you cannot make
  - Once all conflicts are resolved: `git add -A`, then `git push`
- `{"status": "error", ...}` — report the step and message to the user.

### Step 3 — Handle denial

Unstage everything first (files were staged in Round 3 for diff purposes):

```bash
git reset HEAD
```

`git reset HEAD` only unstages — it moves files back from staged to unstaged.
No code is deleted, no changes are lost. It is the opposite of `git add`.

Clean up the message file (written in Round 5 Step 1 before denial):

```bash
rm .flow-commit-msg
```

Then ask: **What needs to be addressed before committing?**

Listen to the reason, acknowledge it clearly, and stop. Do not commit. The user
will make fixes and re-invoke the commit skill when ready.

### Hard Rules

- Never commit without showing the diff first
- The default commit mode is auto — never prompt for approval unless `--manual` was explicitly passed
- `--auto` is user-invoked only. Claude must never call `/flow:flow-commit --auto` programmatically — except in phase skills (`/flow:flow-start`, `/flow:flow-code`, `/flow:flow-code-review`, `/flow:flow-learn`) which commit autonomously as part of their workflow.
- Never use `--no-verify`
- Never add Co-Authored-By trailers or attribution lines — commits are authored by the user alone
- Always pull before pushing — other sessions may have merged changes
- **Never rebase — ever.** Always merge. `git rebase` is forbidden.
- Never discard uncommitted or staged changes — if unexpected changes exist, show `git diff` to the user and ask how to proceed before taking any action

## Additional Rules

- **FLOW mode only:** If `bin/flow ci` has not been run since the last code change, warn the user before asking for approval
