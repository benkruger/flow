---
title: /flow:init
nav_order: 7
parent: Skills
---

# /flow:init

**Phase:** Any (run once per install/upgrade)

**Usage:** `/flow:init`

One-time project setup. Configures workspace permissions in `.claude/settings.json`, sets up git excludes, and writes a version marker. Run once after installing FLOW and again after each upgrade.

---

## What It Does

1. Asks which framework the project uses (Rails or Python)
2. Reads `.claude/settings.json` (or starts with `{}`)
3. Merges FLOW allow/deny permission entries (universal + framework-specific), preserving existing entries
4. Writes the merged `.claude/settings.json`
5. Writes `.flow.json` with the current FLOW version and framework
6. Adds `.flow-states/` and `.worktrees/` to `.git/info/exclude`
7. Commits `.claude/settings.json` and `.flow.json`

---

## Gates

- Must be in a git repository
- Must be on the main branch (permissions are committed and shared with the team)

---

## See Also

- [/flow:start](flow-start.md) — requires `/flow:init` to have been run for the current FLOW version
