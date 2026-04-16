---
name: flow-commit
description: "Review the full diff, then git add + commit + push. Use at every commit checkpoint in the FLOW workflow."
---

# Commit

Review all pending changes as a diff before committing.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Round 1 — Setup

Run `git worktree list --porcelain`. Note the path on the first
`worktree` line (this is the project root). Find the `worktree` entry
whose path matches your current working directory — the
`branch refs/heads/<name>` line in that entry is the current branch
(strip the `refs/heads/` prefix).

Keep the project root and branch in context for the rest of this skill.

## Round 2 — Banner and Format Detection

**Step 1.** Use the Glob tool: pattern `*.json`, path `<project_root>/.flow-states` — if any results, a FLOW phase is active (used for banner selection only).

**Step 2.** If any `.flow-states/*.json` results exist, use the Read tool to read the state file for the current branch at `<project_root>/.flow-states/<branch>.json`.

- Parse `commit_format`: `"title-only"` or `"full"`.
- If no state file exists or the state file has no `commit_format` key → use `"full"`.

Keep `commit_format` in context.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

**If a state file exists (`.flow-states/*.json` Glob returned results):**

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-commit — STARTING
──────────────────────────────────────────────────
```
````

**Otherwise (no state file):**

````markdown
```text
──────────────────────────────────────────────────
  Commit — STARTING
──────────────────────────────────────────────────
```
````

On completion (whether nothing to commit or committed successfully), print the same way:

**If a state file exists:**

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-commit — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

**Otherwise:**

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
```

---

## Process

### Round 3 — Stage

```bash
git add -A
```

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

Display the full message under the heading **Commit Message**.

### Round 5 — Commit and push

Files are already staged from Round 3. No need to `git add -A` again.

1. Use the Write tool to write the commit message to `.flow-commit-msg` in the project root.
   - Each worktree has its own project root, so concurrent sessions don't collide
   - The file is inside the project, so the Write tool has permission without prompting
   - The Write tool handles newlines and special characters safely — no shell escaping needed
   - Never write to `/tmp/` — paths outside the project trigger permission prompts that settings.json cannot suppress
   - Never use `python3 -c` to write the message — literal `$(...)` in the body triggers command substitution warnings
   - Never use `git commit -m` with heredoc — the multi-line command fails permission pattern matching

### Round 6 — Finalize

Run the finalize script to commit, clean up the message file, pull,
and push in one call. `finalize-commit` runs `ci::run_impl()` before
`git commit` (see CLAUDE.md "CI is enforced inside `finalize-commit`
itself"), so use a 10-minute Bash tool timeout (`timeout: 600000`) —
CI runs can take 3–4 minutes and the default 2-minute timeout would
background the process, defeating the gate (per
`.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow finalize-commit .flow-commit-msg <current-branch>
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

### Hard Rules

- Never commit without showing the diff first
- Never use `--no-verify`
- Never add Co-Authored-By trailers or attribution lines — commits are authored by the user alone
- Always pull before pushing — other sessions may have merged changes
- **Never rebase — ever.** Always merge. `git rebase` is forbidden.
- Never discard uncommitted or staged changes — if unexpected changes exist, show `git diff` to the user and ask how to proceed before taking any action
