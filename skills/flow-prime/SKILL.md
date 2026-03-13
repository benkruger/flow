---
name: flow-prime
description: "One-time project setup — configure workspace permissions, git excludes, and version marker. Run once after installing or upgrading FLOW. Usage: /flow:flow-prime"
---

# FLOW Prime — One-Time Project Setup

## Usage

```text
/flow:flow-prime
```

Run once after installing FLOW, and again after each FLOW upgrade. Configures workspace permissions, git excludes, and writes a version marker so `/flow:flow-start` knows the project is initialized.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.14 — Prime — STARTING
============================================
```
````

## Steps

### Step 1 — Detect framework

Auto-detect the framework using the data-driven detector:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow detect-framework <project_root>
```

Parse the JSON output. The `detected` array contains frameworks matched
by file presence, and `available` lists all supported frameworks.

If exactly one framework is detected, confirm with AskUserQuestion:

> "Detected **<display_name>** project. Is this correct?"
>
> - **Yes, <display_name>** — "Proceed with <display_name> setup"
> - One option per other available framework — "<display_name>"

If no frameworks detected, or multiple detected, ask the user to choose
from the available list using AskUserQuestion with one option per
available framework.

Store the answer as `framework` (lowercase name from the JSON).

### Step 2 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — how `/flow:flow-commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code.
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, code-review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (abort, complete) have a single mode value. The `/flow:flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-plan": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-complete": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "manual", "continue": "manual"}, "flow-learn": {"commit": "manual", "continue": "manual"}, "flow-abort": "manual", "flow-complete": "manual"}
```

**Recommended** — safe defaults for all frameworks:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-complete": "auto"}
```

**Customize** — ask per skill, in this order: start, plan, code, code-review, learn, abort, complete. For each skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

For **code** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-code? (controls diff approval and per-task approval)"
>
> - **Manual (Recommended)** — "Require explicit approval"
> - **Auto** — "Skip approval prompts"

Second question:

> "Continue mode for /flow:flow-code? (controls phase advancement)"
>
> - **Manual (Recommended)** — "Prompt before advancing"
> - **Auto** — "Auto-advance to next phase"

For **code-review** and **learning** (commit and continue), ask two AskUserQuestions each:

First question:

> "Commit mode for /flow:flow-<skill>? (controls diff approval and per-task approval)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-<skill>? (controls phase advancement)"
>
> - **Auto (Recommended)** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

For **start** (continue only), ask one AskUserQuestion:

> "Continue mode for /flow:flow-start?"
>
> - **Manual (Recommended)** — "Prompt before advancing"
> - **Auto** — "Auto-advance to next phase"

For **plan** (continue only), ask one AskUserQuestion:

> "Continue mode for /flow:flow-plan? (controls phase advancement to Code)"
>
> - **Auto (Recommended)** — "Auto-advance to Code phase"
> - **Manual** — "Prompt before advancing"

For **abort** and **complete** (single mode), ask one AskUserQuestion each:

> "Mode for /flow:flow-<skill>?"
>
> - **Auto (Recommended)** — "Skip confirmation prompt"
> - **Manual** — "Require confirmation prompt"

Store the result as `skills_dict` for Step 4.

### Step 3 — Choose commit message format

FLOW supports two commit message formats:

- **Title only** — subject line + file list (minimal, no tl;dr section)
- **Full** — subject + tl;dr + explanation + file list (detailed seven-element format)

Ask the user which format to use with AskUserQuestion:

> "What commit message format should FLOW use?"
>
> - **Title only** — "Subject line + file list, no tl;dr section"
> - **Full format** — "Subject + tl;dr + explanation + file list (detailed)"

Store the result as `commit_format`:

- "Title only" → `"title-only"`
- "Full format" → `"full"`

### Step 4 — Run prime setup script

Serialize `skills_dict` from Step 2 as a JSON string for the `--skills-json` argument.
Pass the `commit_format` value from Step 3 via `--commit-format`.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --framework <framework> --skills-json '<skills_dict_json>' --commit-format <commit_format>
```

The script handles everything in a single call:

- Reading or creating `.claude/settings.json`
- Merging FLOW permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker, framework, config hash, skills config, and commit format
- Adding `.flow-states/`, `.worktrees/`, `.flow.json`, and `bin/dependencies` to `.git/info/exclude`
- Installing a pre-commit hook at `.git/hooks/pre-commit` that blocks direct `git commit` and requires all commits to go through `/flow:flow-commit`
- Priming the project CLAUDE.md with framework conventions (if CLAUDE.md exists)
- Creating `bin/dependencies` from the framework template (skips if already exists)

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true, "hook_installed": true, "framework": "rails|python", "prime_project": "ok|error", "dependencies": "ok|skipped"}`

If the script returns an error, show the message and stop.

The `config_hash` field in `.flow.json` is a 12-character hex digest. When the plugin version changes, `/flow-start` recomputes the hash and compares against the stored value to decide whether re-prime is needed. If the config hasn't changed, the version is auto-upgraded without re-running `/flow-prime`.

The permissions merged depend on the framework. Universal permissions are
always merged. Framework-specific permissions are loaded from
`frameworks/<name>/permissions.json` and added based on the chosen framework.

All permissions (universal + all framework sets) for reference:

```json
{
  "permissions": {
    "allow": [
      "Bash(git -C *)",
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push -u *)",
      "Bash(git reset HEAD)",
      "Bash(git worktree *)",
      "Bash(git pull origin *)",
      "Bash(git status)",
      "Bash(git diff)",
      "Bash(git diff *)",
      "Bash(git log *)",
      "Bash(git branch --show-current)",
      "Bash(cd *)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(git push origin --delete *)",
      "Bash(git branch -D *)",
      "Bash(bin/*)",
      "Bash(rm .flow-commit-*)",
      "Bash(rm .claude/settings.local.json)",
      "Bash(*bin/flow *)",
      "Bash(gh pr view *)",
      "Bash(bin/rails test *)",
      "Bash(rubocop *)",
      "Bash(rubocop -A)",
      "Bash(bundle update --all)",
      "Bash(bundle exec *)",
      "Bash(psql *)",
      "Bash(.venv/bin/pip install *)",
      "Bash(git restore *)",
      "Bash(git fetch origin *)",
      "Bash(git merge *)",
      "Bash(gh pr checks *)",
      "Bash(gh pr merge *)",
      "Bash(claude plugin list)",
      "Bash(claude plugin marketplace add *)",
      "Bash(claude plugin install *)",
      "Bash(gh issue list *)",
      "Bash(gh issue view *)",
      "Read(~/.claude/rules/*)",
      "Read(/tmp/*.txt)"
    ],
    "deny": [
      "Bash(git rebase *)",
      "Bash(git push --force *)",
      "Bash(git push -f *)",
      "Bash(git reset --hard *)",
      "Bash(git stash *)",
      "Bash(git checkout *)",
      "Bash(git clean *)",
      "Bash(* && *)",
      "Bash(* ; *)",
      "Bash(* | *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 5 — Install code-review plugin

Check if the `code-review` plugin is already available:

```bash
claude plugin list
```

If the output does not contain `claude-code-plugins`, add the marketplace source:

```bash
claude plugin marketplace add anthropics/claude-code
```

If the output does not contain `code-review`, install it:

```bash
claude plugin install code-review@claude-code-plugins
```

If both are already present, skip silently.

### Step 6 — Commit and push

Check if anything is staged by running `git status`. If the output contains "nothing to commit", skip the commit and push — go straight to Done.

Otherwise, commit via `/flow:flow-commit`.

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.14 — Prime — COMPLETE
============================================
```
````

Report:

- Framework: `<framework>`
- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json` (git-excluded)
- Git excludes configured for `.flow-states/`, `.worktrees/`, `.flow.json`, and `bin/dependencies`
- Pre-commit hook installed — blocks direct `git commit`, requires `/flow:flow-commit`
- Changes committed

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start       | —      | manual   |
| plan        | —      | auto     |
| code        | manual | manual   |
| code-review | auto   | auto     |
| learning    | auto   | auto     |
| abort       | auto   | —        |
| complete    | auto   | —        |
```

Use the actual values from `skills_dict` (Step 2). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow-start <feature name>`.
