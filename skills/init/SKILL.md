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
  FLOW v0.8.1 — Init — STARTING
============================================
```
````

## Steps

### Step 1 — Read current settings

Read `.claude/settings.json` using the Read tool. If the file does not exist, start with an empty object `{}`.

### Step 2 — Merge FLOW permissions

Merge the FLOW permission entries into the settings object from Step 1. Preserve all existing entries. Only add entries that do not already exist. Set `defaultMode` to `acceptEdits` if `defaultMode` is not already set.

The FLOW permissions to merge:

```json
{
  "permissions": {
    "allow": [
      "Bash(cd .worktrees/* && *)",
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push; *)",
      "Bash(git push -u *)",
      "Bash(git reset HEAD)",
      "Bash(git reset HEAD; *)",
      "Bash(git worktree *)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(git push origin --delete *)",
      "Bash(git branch -D *)",
      "Bash(bin/ci)",
      "Bash(bin/ci; *)",
      "Bash(bin/rails test *)",
      "Bash(rubocop *)",
      "Bash(rubocop -A)",
      "Bash(bundle update --all)",
      "Bash(bundle update --all; *)",
      "Bash(rm .flow-commit-*)",
      "Bash(bundle exec *)"
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

### Step 3 — Write merged settings

Create the `.claude/` directory if it does not exist. Write the merged settings to `.claude/settings.json` using the Write tool.

### Step 4 — Write version marker

Write `.flow.json` in the project root using the Write tool:

```json
{"flow_version": "0.7.3"}
```

This file tells `/flow:start` that the project has been initialized for this FLOW version.

### Step 5 — Configure git exclude

Run:

```bash
git rev-parse --git-common-dir
```

Read the `info/exclude` file at that path using the Read tool (empty string if it does not exist).

If `.flow-states/` is not already in the file, add it. If `.worktrees/` is not already in the file, add it. Write the updated file using the Edit tool (or Write if the file is new).

### Step 6 — Commit

Stage and commit the settings and version marker:

```bash
git add .claude/settings.json .flow.json
```

```bash
git commit -m "Configure FLOW workspace permissions and version marker"
```

### Done — Complete

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.8.1 — Init — COMPLETE
============================================
```
````

Report:

- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json`
- Git excludes configured for `.flow-states/` and `.worktrees/`
- Changes committed

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow:start <feature name>`.
