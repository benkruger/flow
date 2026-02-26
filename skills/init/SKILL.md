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
  FLOW v0.8.5 — Init — STARTING
============================================
```
````

## Steps

### Step 1 — Run init setup script

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow init-setup <project_root>
```

The script handles:

- Reading or creating `.claude/settings.json`
- Merging FLOW permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` if not already set
- Writing `.flow.json` version marker
- Adding `.flow-states/` and `.worktrees/` to `.git/info/exclude`

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true}`

If the script returns an error, show the message and stop.

The FLOW permissions merged by the script:

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
      "Bash(bundle exec *)",
      "Bash(*bin/flow *)"
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

### Step 2 — Commit and push

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
  FLOW v0.8.5 — Init — COMPLETE
============================================
```
````

Report:

- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json`
- Git excludes configured for `.flow-states/` and `.worktrees/`
- Changes committed

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow:start <feature name>`.
