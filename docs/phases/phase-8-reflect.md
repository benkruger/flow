---
title: "Phase 8: Reflect"
nav_order: 10
---

# Phase 8: Reflect

**Command:** `/flow:reflect`

Runs before the PR is merged. Reviews what went wrong across all phases,
proposes learnings, routes each to its correct permanent home, and notes
plugin gaps. The only commits are CLAUDE.md and `.claude/` changes —
application code is never touched.

---

## Four Sources

Reflect synthesises from all four before asking the user anything:

1. **State file data** — visit counts, timing, captured `/flow:note` entries, research risks, open questions
2. **Captured notes** — corrections logged automatically by `/flow:note` throughout the session
3. **Conversation context** — what Claude can still see of the session's back-and-forth
4. **Worktree auto-memory** — patterns and observations Claude wrote to auto-memory during feature work, which will be lost when Cleanup removes the worktree

Sources 1, 2, and 4 survive compaction. Context is a bonus if available.

---

## What Gets Captured

Each approved learning is routed to the destination where it belongs:

| Destination | What goes here | Write method |
|---|---|---|
| Global CLAUDE.md | Process rules for all projects | Direct edit (private) |
| Project CLAUDE.md | Project-specific architecture | Committed via PR |
| Global rules | Universal coding standards | Direct edit (private) |
| Project rules | Project-specific coding gotchas | Committed via PR |
| Project memory | Patterns and observations | Direct edit (private) |

Claude recommends a destination for each learning based on content type.
The user confirms or overrides with one click.

**Plugin improvement notes** — presented only, never committed:
- Places where the FLOW process itself should improve
- User decides whether to open issues on the plugin repo

---

## What Makes a Good CLAUDE.md Entry

**Good:** Generic pattern that prevents the same mistake in any future feature
> "Never assume branch-behind is unlikely in a multi-session workflow"

**Bad:** Feature-specific note that only applies here
> "The payments module uses a specific queue configuration"

---

## What Comes Next

Merge the PR manually (which now includes CLAUDE.md improvements),
then run Phase 9: Cleanup (`/flow:cleanup`).
