---
title: "Phase 7: Reflect"
nav_order: 8
---

# Phase 7: Reflect

**Command:** `/flow:reflect`

Runs before the PR is merged. Autonomously reviews what went wrong across
all phases, routes learnings to their correct permanent homes, files GitHub
issues for plugin improvements, and presents a comprehensive report. The
only commits are CLAUDE.md and `.claude/` changes — application code is
never touched.

---

## Four Sources

Reflect synthesises from all four before taking any action:

1. **State file data** — visit counts, timing, captured `/flow:note` entries, plan file risks
2. **Captured notes** — corrections logged automatically by `/flow:note` throughout the session
3. **Conversation context** — what Claude can still see of the session's back-and-forth
4. **Worktree auto-memory** — patterns and observations Claude wrote to auto-memory during feature work, which will be lost when Cleanup removes the worktree

Sources 1, 2, and 4 survive compaction. Context is a bonus if available.

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
- One issue per process gap, labeled `reflect`

---

## What Makes a Good CLAUDE.md Entry

**Good:** Generic pattern that prevents the same mistake in any future feature
> "Never assume branch-behind is unlikely in a multi-session workflow"

**Bad:** Feature-specific note that only applies here
> "The payments module uses a specific queue configuration"

---

## Three Modes

Reflect auto-detects its context and adjusts behavior:

| Mode | Trigger | Sources | Commits | Settings audit | GitHub issues |
|------|---------|---------|---------|----------------|---------------|
| Phase 7 | State file with Security complete | 4 (state, notes, context, worktree memory) | `/flow:commit --auto` | No | Yes |
| Maintainer | No state file, `flow-phases.json` exists | 2 (CLAUDE.md, context) | `/flow:commit --auto` | Yes | No |
| Standalone | No state file, no `flow-phases.json` | 2 (CLAUDE.md, context) | None | No | No |

All three modes edit the same 5 destinations on disk. Stealth users
(who exclude `.claude/` from git) are safe — git's own exclusion
mechanism prevents excluded files from being staged.

---

## What Comes Next

Merge the PR manually (which now includes CLAUDE.md improvements),
then run Phase 8: Cleanup (`/flow:cleanup`).
