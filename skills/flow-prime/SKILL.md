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
  FLOW v0.20.1 — Prime — STARTING
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

Phase skills that commit (code, code-review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (abort, cleanup) have a single mode value. The `/flow:flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "manual", "continue": "manual"}, "flow-learning": {"commit": "manual", "continue": "manual"}, "flow-abort": "manual", "flow-cleanup": "manual"}
```

**Recommended** — safe defaults for all frameworks:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}
```

**Customize** — ask per skill, in this order: start, code, code-review, learning, abort, cleanup. For each skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

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

For **abort** and **cleanup** (single mode), ask one AskUserQuestion each:

> "Mode for /flow:flow-<skill>?"
>
> - **Auto (Recommended)** — "Skip confirmation prompt"
> - **Manual** — "Require confirmation prompt"

Store the result as `skills_dict` for Step 3.

### Step 3 — Run prime setup script

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --framework <framework>
```

The script handles:

- Reading or creating `.claude/settings.json`
- Merging FLOW permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker and framework
- Adding `.flow-states/`, `.worktrees/`, and `.flow.json` to `.git/info/exclude`

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true, "framework": "rails|python"}`

If the script returns an error, show the message and stop.

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
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(git push origin --delete *)",
      "Bash(git branch -D *)",
      "Bash(bin/ci)",
      "Bash(bin/dependencies)",
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

After the prime-setup script writes `.flow.json`, read it back with the Read tool,
add the `skills` key from `skills_dict` (Step 2), and write the file back with
the Write tool. The result should look like:

```json
{"flow_version": "0.16.4", "framework": "python", "config_hash": "2c54c5cd6972", "skills": {"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learning": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-cleanup": "auto"}}
```

The `config_hash` field is a 12-character hex digest stored by `prime-setup`. When the plugin version changes, `/flow-start` recomputes the hash and compares against the stored value to decide whether re-prime is needed. If the config hasn't changed, the version is auto-upgraded without re-running `/flow-prime`.

### Step 5 — Prime project CLAUDE.md

If the project has a `CLAUDE.md`, prime it with framework conventions:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow prime-project <project_root> --framework <framework>
```

Parse the JSON output. If `"status": "ok"`, the project CLAUDE.md now
contains framework conventions between `<!-- FLOW:BEGIN -->` and
`<!-- FLOW:END -->` markers. If `"status": "error"`, skip priming
silently — the user can prime later by running init again after
creating a CLAUDE.md.

### Step 6 — Create bin/dependencies

Create the dependency updater script from the framework template:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow create-dependencies <project_root> --framework <framework>
```

Parse the JSON output. If `"status": "ok"`, `bin/dependencies` was
created. If `"status": "skipped"`, the file already exists (user may
have customized it). If `"status": "error"`, report to the user.

### Step 7 — Commit and push

Stage the settings and version marker:

Stage the settings file:

```bash
git add .claude/settings.json
```

Check if anything is staged by running `git status`. If the output contains "nothing to commit", skip the commit and push — go straight to Done.

Otherwise, commit and push:

```bash
git commit -m "Configure FLOW workspace permissions"
```

```bash
git push
```

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.20.1 — Prime — COMPLETE
============================================
```
````

Report:

- Framework: `<framework>`
- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json` (git-excluded)
- Git excludes configured for `.flow-states/`, `.worktrees/`, and `.flow.json`
- Changes committed

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start       | —      | manual   |
| code        | manual | manual   |
| code-review | auto   | auto     |
| learning    | auto   | auto     |
| abort       | auto   | —        |
| cleanup     | auto   | —        |
```

Use the actual values from `skills_dict` (Step 2). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow-start <feature name>`.
