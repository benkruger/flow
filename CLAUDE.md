# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A Claude Code plugin (`flow:` namespace) implementing an opinionated Rails development lifecycle. Every feature follows the same 8 phases in the same order.

## Installation (in a Rails project)

```
/plugin marketplace add benkruger/ruby-on-rails-claude-ai-process
/plugin install flow@ruby-on-rails-claude-ai-process
```

## The 8 Phases

```
1 Start → 2 Research → 3 Design → 4 Plan → 5 Code → 6 Review → 7 Reflect → 8 Cleanup
```

| Phase | Command | Status |
|-------|---------|--------|
| 1: Start | `/flow:start <name>` | Built |
| 2: Research | `/flow:research` | Built |
| 3: Design | `/flow:design` | Built |
| 4: Plan | `/flow:plan` | Built |
| 5: Code | `/flow:code` | Built |
| 6: Review | `/flow:review` | Built |
| 7: Reflect | `/flow:reflect` | Built |
| 8: Cleanup | `/flow:cleanup` | Built |

## Utility Skills

| Skill | Purpose |
|-------|---------|
| `/flow:commit` | Review diff, approve/deny, pull, commit, push |
| `/flow:status` | Show current phase, timing, PR link |
| `/flow:resume` | Resume mid-session or new session |
| `/flow:note` | Capture correction/learning — auto-invoked on corrections |

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects resume context
- `hooks/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (enable in Settings → Pages → main /docs)

## State File

Lives in `.claude/flow-states/<branch>.json` in the target Rails project.
Gitignored. Tracks: phase status, timing, visit counts, research findings,
design decisions, plan tasks, and captured notes.

## What Still Needs Work

- Test the plugin installation in a real Rails project
- Enable GitHub Pages on the repo
- The `flow-phases.json` `can_return_to` values may need tuning after real use
- Each phase skill should also ask for an optional note at phase transitions (pattern is documented in `docs/reference/skill-pattern.md` but not yet updated in all skills)

## Conventions for This Repo

- All commits via `/flow:commit` skill
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- Never disable RuboCop or modify `.rubocop.yml` without user approval
