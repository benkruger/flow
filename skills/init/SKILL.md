---
name: init
description: "One-time project setup — configure workspace permissions, git excludes, and version marker. Run once after installing or upgrading FLOW. Usage: /flow:init"
---

# FLOW Init — One-Time Project Setup

## Usage

```text
/flow:init
```

Run once after installing FLOW, and again after each FLOW upgrade. Configures workspace permissions, git excludes, and writes a version marker so `/flow:start` knows the project is initialized.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.15.0 — Init — STARTING
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

### Step 2 — Run init setup script

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
      "Bash(cd .worktrees/* && *)",
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
      "Bash(git clean *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 3 — Commit and push

Stage and commit the settings and version marker:

```bash
git add .claude/settings.json .flow.json
```

If `git status` shows nothing staged (re-run, no changes), skip the commit and push — print "Already initialized, no changes needed." and go to Done.

```bash
git commit -m "Configure FLOW workspace permissions and version marker"
```

```bash
git push
```

### Done — Complete

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.15.0 — Init — COMPLETE
============================================
```
````

Report:

- Framework: `<framework>`
- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json`
- Git excludes configured for `.flow-states/` and `.worktrees/`
- Changes committed

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow:start <feature name>`.
