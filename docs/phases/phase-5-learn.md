---
title: "Phase 5: Learn"
nav_order: 6
---

# Phase 5: Learn

**Command:** `/flow-learn`

Runs before the PR is merged. Autonomously reviews what went wrong across
all phases, routes learnings to their correct permanent homes, files GitHub
issues for plugin improvements, and presents a comprehensive report. The
only commits are CLAUDE.md and `.claude/` changes — application code is
never touched.

---

## Three Sources

Learn synthesises from all three before taking any action:

1. **CLAUDE.md rules** — the project's rules and conventions that should have been followed
2. **Conversation context** — what Claude can still see of the session's back-and-forth
3. **State file and plan data** — visit counts, timing, captured `/flow-note` entries, plan file risks (Phase 5 only)

Sources 1 and 3 survive compaction. Context is a bonus if available.

---

## What Gets Captured

Claude decides destinations autonomously using content-type heuristics:

| Destination | What goes here | Write method |
|---|---|---|
| Global CLAUDE.md | Process rules for all projects | Direct edit (private) |
| Project CLAUDE.md | Project-specific architecture | Committed via PR |
| Global rules | Universal coding standards | Direct edit (private) |
| Project rules | Project-specific coding gotchas | Committed via PR |
| Project memory | Patterns and observations | Direct edit (private) |

**Plugin improvement notes** — filed as GitHub issues on the plugin repo:
- Places where the FLOW process itself should improve
- One issue per process gap, labeled `learning`

---

## What Makes a Good CLAUDE.md Entry

**Good:** Generic pattern that prevents the same mistake in any future feature
> "Never assume branch-behind is unlikely in a multi-session workflow"

**Bad:** Feature-specific note that only applies here
> "The payments module uses a specific queue configuration"

---

## Three Modes

Learn auto-detects its context and adjusts behavior:

| Mode | Trigger | Sources | Commits | Settings audit | GitHub issues |
|------|---------|---------|---------|----------------|---------------|
| Phase 5 | State file with Code Review complete | 3 (CLAUDE.md, context, state/plan) | `/flow-commit --auto` | No | Yes |
| Maintainer | No state file, `flow-phases.json` exists | 2 (CLAUDE.md, context) | `/flow-commit --auto` | Yes | No |
| Standalone | No state file, no `flow-phases.json` | 2 (CLAUDE.md, context) | None | No | No |

All three modes route learnings to the same 5 destinations, split into
**instructions** (destinations 1-4, always loaded) and **context**
(destination 5, informational). Stealth users (who exclude `.claude/`
from git) are safe — git's own exclusion mechanism prevents excluded
files from being staged.

---

## What Comes Next

Merge the PR manually (which now includes CLAUDE.md improvements),
then run Phase 6: Cleanup (`/flow-cleanup`).
