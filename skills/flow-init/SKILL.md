---
name: flow-init
description: "One-time project setup — configure workspace permissions, git excludes, and version marker. Run once after installing or upgrading FLOW. Usage: /flow:flow-init"
---

# FLOW Init — One-Time Project Setup

## Usage

```text
/flow:flow-init
```

Run once after installing FLOW, and again after each FLOW upgrade. Configures workspace permissions, git excludes, and writes a version marker so `/flow:flow-start` knows the project is initialized.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.1 — Init — STARTING
============================================
```
````

## Steps

### Step 1 — Detect framework

Auto-detect the framework from project files:

1. Use the Glob tool to check for `Gemfile` at the project root — Rails indicator
2. Use the Glob tool to check for `pyproject.toml`, `setup.py`, or `requirements.txt` at the project root — Python indicator

If exactly one framework is detected, confirm with AskUserQuestion:

- If Rails detected: "Detected **Rails** project (found Gemfile). Is this correct?"
  - Option 1: **Yes, Rails** — "Proceed with Rails setup"
  - Option 2: **No, it's Python** — "Use Python setup instead"

- If Python detected: "Detected **Python** project (found pyproject.toml/setup.py/requirements.txt). Is this correct?"
  - Option 1: **Yes, Python** — "Proceed with Python setup"
  - Option 2: **No, it's Rails** — "Use Rails setup instead"

If no framework files detected, or both detected, fall back to asking:

- Question: "What framework does this project use?"
- Option 1: **Rails** — "Ruby on Rails project"
- Option 2: **Python** — "Python project"

Store the answer as `framework` (lowercase: `rails` or `python`).

### Step 2 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — how `/flow:flow-commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code and refactoring approval in Simplify.
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, simplify, review, security, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (abort, cleanup) have a single mode value. The `/flow:flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-simplify": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-security": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-simplify": {"commit": "manual", "continue": "manual"}, "flow-review": {"commit": "manual", "continue": "manual"}, "flow-security": {"commit": "manual", "continue": "manual"}, "flow-learning": {"commit": "manual", "continue": "manual"}, "flow-abort": "manual", "flow-cleanup": "manual"}
```

**Recommended** — framework-aware defaults:

For Rails:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-simplify": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "manual", "continue": "auto"}, "flow-security": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}
```

For Python:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-simplify": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-security": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}
```

**Customize** — ask per skill, in this order: start, code, simplify, review, security, learning, abort, cleanup. For each skill, ask about only the applicable axes:

For skills with both axes (code, simplify, review, security, learning), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-<skill>? (controls diff approval and per-task approval)"
>
> - **Auto** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-<skill>? (controls phase advancement)"
>
> - **Auto** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

For skills with continue only (start), ask one AskUserQuestion:

> "Continue mode for /flow:flow-<skill>?"
>
> - **Auto** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

For utility skills (abort, cleanup), ask one AskUserQuestion:

> "Mode for /flow:flow-<skill>?"
>
> - **Auto** — "Skip confirmation prompt"
> - **Manual** — "Require confirmation prompt"

Store the result as `skills_dict` for Step 3.

### Step 3 — Run init setup script

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow init-setup <project_root> --framework <framework>
```

The script handles:

- Reading or creating `.claude/settings.json`
- Merging FLOW permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker and framework
- Adding `.flow-states/` and `.worktrees/` to `.git/info/exclude`

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true, "framework": "rails|python"}`

If the script returns an error, show the message and stop.

The permissions merged depend on the framework. Universal permissions are always merged. Framework-specific permissions are added based on the chosen framework.

**Universal** (always merged): git operations, worktree management, PR lifecycle, bin/ci, bin/flow

**Rails** (when framework is rails): bin/rails test, rubocop, bundle, psql

**Python** (when framework is python): bin/test

All permissions (universal + both framework sets) for reference:

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
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(git push origin --delete *)",
      "Bash(git branch -D *)",
      "Bash(bin/ci)",
      "Bash(rm .flow-commit-*)",
      "Bash(rm .claude/settings.local.json)",
      "Bash(*bin/flow *)",
      "Bash(gh pr view *)",
      "Bash(gh issue create *)",
      "Bash(bin/rails test *)",
      "Bash(rubocop *)",
      "Bash(rubocop -A)",
      "Bash(bundle update --all)",
      "Bash(bundle exec *)",
      "Bash(psql *)",
      "Bash(bin/test *)",
      "Bash(.venv/bin/pip install *)",
      "Bash(git restore *)"
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
      "Bash(* ; *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 4 — Write skills config to .flow.json

After the init-setup script writes `.flow.json`, read it back with the Read tool,
add the `skills` key from `skills_dict` (Step 2), and write the file back with
the Write tool. The result should look like:

```json
{"flow_version": "0.16.4", "framework": "python", "skills": {"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-simplify": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-security": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}}
```

### Step 5 — Commit and push

Stage the settings and version marker:

```bash
git add .claude/settings.json .flow.json
```

Check if anything is staged by running `git status`. If the output contains "nothing to commit", skip the commit and push — go straight to Done.

Otherwise, commit and push:

```bash
git commit -m "Configure FLOW workspace permissions and version marker"
```

```bash
git push
```

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.1 — Init — COMPLETE
============================================
```
````

Report:

- Framework: `<framework>`
- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json`
- Git excludes configured for `.flow-states/` and `.worktrees/`
- Changes committed

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start     | —      | manual   |
| code      | manual | manual   |
| simplify  | auto   | auto     |
| review    | auto   | auto     |
| security  | auto   | auto     |
| learning  | auto   | auto     |
| abort     | auto   | —        |
| cleanup   | auto   | —        |
```

Use the actual values from `skills_dict` (Step 2). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow-start <feature name>`.
