---
name: flow-commit
description: "Review the full diff, then git add + commit + push. Use at every commit checkpoint in the FLOW workflow."
---

# Commit

Review all pending changes as a diff before committing.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
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

## Round 2 — Banner Detection

Use the Glob tool: pattern `*.json`, path `<project_root>/.flow-states` — if any results, a FLOW phase is active (used for banner selection only).

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

**If a state file exists (`.flow-states/*.json` Glob returned results):**

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.6.1 — flow:flow-commit — STARTING
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
  ✓ FLOW v2.6.1 — flow:flow-commit — COMPLETE
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

FLOW commits follow the [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification so downstream CHANGELOG tooling matches every merge. There is one format — no per-project choice.

Structure:

```text
type(scope): description

Free-form body paragraph explaining the WHY — what problem this solves,
what behaviour changes, or what was wrong before.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
- path/to/another.rb: What changed and why

BREAKING CHANGE: description of the incompatible change
```

**Subject line.**

- Shape: `type(scope): description`. The `type` is required; the `(scope)` is optional and omitted unless one obvious scope applies.
- `description` is lowercase, imperative mood, and has no trailing period.
- Infer `type` from the diff, using the standard set:
  - `feat` — a new user-facing capability (minor version bump)
  - `fix` — a bug fix (patch version bump)
  - `docs` — documentation only
  - `refactor` — a change that neither fixes a bug nor adds a feature
  - `perf` — a performance improvement
  - `test` — adding or correcting tests only
  - `build` — build system or dependency changes
  - `ci` — CI configuration changes
  - `chore` — anything else that does not change src or test behaviour
- Capture the business reason in the description or body — why this change matters, not just what changed.

**Body.**

- Blank line between the subject and the body.
- A free-form paragraph explaining the motivation — what prompted this change.
- A per-file bullet list: one bullet per changed file with a plain-English reason. Call out explicitly if the diff includes migrations, schema changes, or dependency-manifest changes.
- Do not pad with obvious restatements of the diff.

**Breaking changes.**

- When the diff introduces a backwards-incompatible change, append a `BREAKING CHANGE: <description>` footer (or mark the type with `!`, e.g. `feat!: ...`). This drives the major-version bump in CHANGELOG tooling. Omit the footer entirely when nothing breaks.

Before displaying your draft, verify it contains all of these in order:

1. Subject line — `type(scope): description`, lowercase imperative description, no trailing period
2. Blank line
3. Body paragraph — the WHY, not the what
4. Blank line
5. File list — one bullet per changed file with reason
6. A `BREAKING CHANGE:` footer ONLY when the diff is backwards-incompatible

If any element is missing or out of order, rewrite before displaying.

Display the full message under the heading **Commit Message**.

### Round 5 — Commit and push

Files are already staged from Round 3. `bin/flow finalize-commit` runs
its CI gate next and then re-stages tracked-file modifications (via
`git add -u`) before composing the commit, so any in-place changes
the project's `bin/*` tools made to already-tracked files during CI
(running in their default non-`CI=1` mode) are captured in the same
commit alongside the manually-staged content. Untracked files are NOT
swept by the re-stage — only modifications to files already tracked
by git.

Use the Write tool to write the commit message content to
`<project_root>/.flow-states/<branch>/commit-msg-content.txt` — a
branch-scoped temp path, not the final commit-msg file. This avoids
Claude Code's Write-tool preflight tripping on a pre-existing final
file from a prior commit retry (see
`.claude/rules/file-tool-preflights.md`).

- The file is inside the project, so the Write tool has permission without prompting
- The Write tool handles newlines and special characters safely — no shell escaping needed
- Never write to `/tmp/` — paths outside the project trigger permission prompts that settings.json cannot suppress
- Never use `python3 -c` to write the message — literal `$(...)` in the body triggers command substitution warnings
- Never use `git commit -m` with heredoc — the multi-line command fails permission pattern matching

Route the content to the final commit-msg file via `bin/flow
write-rule`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <project_root>/.flow-states/<branch>/commit-msg.txt --content-file <project_root>/.flow-states/<branch>/commit-msg-content.txt
```

Both files live inside the per-branch subdirectory
`.flow-states/<branch>/` (alongside `state.json`, `plan.md`, etc.) so
concurrent flows in different worktrees of the same repo never collide
on a single shared file, and `flow-abort`/`flow-complete` cleanup
removes the whole subdirectory in one `remove_dir_all` call.
`finalize-commit` reads and deletes the final commit-msg file
unchanged by this routing.

### Round 6 — Finalize

Run the finalize script to commit, clean up the message file, pull,
and push in one call. `finalize-commit` runs `ci::run_impl()` before
`git commit` (see CLAUDE.md "CI is enforced inside `finalize-commit`
itself"), so use a 10-minute Bash tool timeout (`timeout: 600000`) —
CI runs can take 3–4 minutes and the default 2-minute timeout would
background the process, defeating the gate (per
`.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow finalize-commit <project_root>/.flow-states/<current-branch>-commit-msg.txt <current-branch>
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
