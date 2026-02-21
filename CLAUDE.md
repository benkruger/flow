# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase Rails development lifecycle. Skills live in `skills/<name>/SKILL.md`. State lives in `.claude/flow-states/<branch>.json` in the target Rails project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects resume context
- `hooks/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (enable in Settings → Pages → main /docs)

## What Still Needs Work

- Test the plugin installation in a real Rails project
- Enable GitHub Pages on the repo
- The `flow-phases.json` `can_return_to` values may need tuning after real use

## Maintainer Skills (private to this repo)

- `/release` — `.claude/skills/release/SKILL.md` — bump version, tag, push, create GitHub Release

## Conventions

- All commits via `/flow:commit` skill
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- Never disable RuboCop or modify `.rubocop.yml` without user approval
