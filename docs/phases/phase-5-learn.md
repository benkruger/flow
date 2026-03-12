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
| Project CLAUDE.md | Process rules and project architecture | Edit on disk, committed via PR |
| `.claude/rules/` | Coding anti-patterns and gotchas | Filed as "Rule" issue (not edited directly) |

Rules that would previously be written to `.claude/rules/` are now filed as
GitHub issues with the "Rule" label. The issue body contains the full rule
text, target file path, and whether it is new or an update. This prevents
permission prompts that break autonomous flow.

**GitHub issues** — filed during Learn:

- **Rule** issues — rule additions/updates for `.claude/rules/`, deferred to a future session
- **Flow** issues — FLOW process gaps, filed on the plugin repo (`benkruger/flow`)
- **Documentation Drift** issues — docs out of sync with actual behavior

All filed issues are recorded in the state file via `bin/flow add-issue`
and surfaced in the Complete phase.

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

All three modes route learnings to 2 repo-local destinations: Project
CLAUDE.md and project rules. Both are committed to the repo. Stealth
users (who exclude `.claude/` from git) are safe — git's own exclusion
mechanism prevents excluded files from being staged.

---

## What Comes Next

Run Phase 6: Complete (`/flow-complete`) to merge the PR (which now
includes CLAUDE.md improvements) and clean up.
