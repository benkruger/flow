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
  FLOW v0.16.4 — Init — STARTING
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

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All 9 configurable skills run in auto mode"
> - **Fully manual** — "All 9 configurable skills run in manual mode"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill"

**Fully autonomous** — set all 9 skills to `auto`:

```json
{"start": "auto", "code": "auto", "simplify": "auto", "review": "auto", "security": "auto", "reflect": "auto", "commit": "auto", "abort": "auto", "cleanup": "auto"}
```

**Fully manual** — set all 9 skills to `manual`:

```json
{"start": "manual", "code": "manual", "simplify": "manual", "review": "manual", "security": "manual", "reflect": "manual", "commit": "manual", "abort": "manual", "cleanup": "manual"}
```

**Recommended** — framework-aware defaults:

For Rails:

```json
{"start": "manual", "code": "manual", "simplify": "manual", "review": "manual", "security": "auto", "reflect": "auto", "commit": "manual", "abort": "auto", "cleanup": "auto"}
```

For Python:

```json
{"start": "manual", "code": "manual", "simplify": "manual", "review": "manual", "security": "auto", "reflect": "auto", "commit": "auto", "abort": "auto", "cleanup": "auto"}
```

**Customize** — ask one AskUserQuestion per skill (9 total), in this order: start, code, simplify, review, security, reflect, commit, abort, cleanup. For each:

> "Mode for /flow:<skill>?"
>
> - **Auto** — "<description of what auto skips>"
> - **Manual** — "Requires explicit approval"

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
      "Bash(git clean *)"
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
{"flow_version": "0.16.4", "framework": "python", "skills": {"start": "manual", "code": "manual", "simplify": "manual", "review": "manual", "security": "auto", "reflect": "auto", "commit": "auto", "abort": "auto", "cleanup": "auto"}}
```

### Step 5 — Commit and push

Stage the settings and version marker:

```bash
git add .claude/settings.json .flow.json
```

Check if anything is staged:

```bash
git diff --cached --quiet
```

If exit code is 0, nothing is staged — skip the commit and push, go straight to Done.

If exit code is 1, changes are staged — commit and push:

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
  FLOW v0.16.4 — Init — COMPLETE
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
