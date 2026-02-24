# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase Rails development lifecycle. Skills live in `skills/<name>/SKILL.md`. State lives in `.claude/flow-states/<branch>.json` in the target Rails project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects resume context
- `hooks/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)

## What Still Needs Work

- The `flow-phases.json` `can_return_to` values may need tuning after real use

## Maintainer Skills (private to this repo)

- `/commit` — `.claude/skills/commit` — review diff, approve, commit, push
- `/release` — `.claude/skills/release/SKILL.md` — bump version, tag, push, create GitHub Release

## Conventions

- All commits via `/commit` skill — no exceptions, no shortcuts, no "just this once"
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)

## Lessons Learned

- **Never bypass `/commit`** — even when the change is small, even when you just used it two commits ago. "All commits via `/commit` skill" is not a guideline, it is a rule. The user had to interrupt mid-commit to stop this.
- **When fixing mistakes, propose the safe variant first** — `git reset --soft` is safe (keeps changes staged). Bare `git reset` is forbidden. Always specify `--soft` and explain why it's non-destructive before asking permission.
- **Consistency audits require comparing the canonical source first** — When reconciling README and docs, start by identifying the canonical example (the marketing page) and grep for every divergence. Do not edit piecemeal and hope you caught everything. The most obvious inconsistency (different feature example names across files) was the one missed.
- **Verify edits against the source of truth before saving** — When fixing an ordering issue, re-read the SKILL.md to confirm the correct order before writing the edit. Editing from memory introduced the exact error being fixed.
