---
title: /flow-learn
nav_order: 9
parent: Skills
---

# /flow-learn

**Phase:** 5 — Learn

**Usage:** `/flow-learn`, `/flow-learn --auto`, or `/flow-learn --manual`

Autonomously synthesises what went wrong from three sources, routes each
learning to its correct permanent home, files GitHub issues for plugin
improvements, and presents a comprehensive report. Runs before the PR
merges.

---

## Sources

| Source | What | Survives compaction? |
|--------|------|---------------------|
| CLAUDE.md rules | Project rules and conventions that should have been followed | Yes |
| Conversation context | Session back-and-forth | Only if not compacted |
| State file and plan data | Visit counts, timing, notes, plan risks (Phase 5 only) | Yes |

---

## Outputs

Learnings are routed autonomously to one of 2 repo-local destinations:

| # | Destination | Path |
|---|-------------|------|
| 1 | Project CLAUDE.md | `CLAUDE.md` in worktree |
| 2 | Project rules | `.claude/rules/<topic>.md` in worktree |

Both destinations are committed to the feature branch via `/flow-commit --auto`.

**Plugin improvement notes** — filed as GitHub issues:

- One issue per process gap on the plugin repo, labeled `learning`
- Issue body describes the gap generically (no user project details)

**Report** — presented after all changes are applied:

- Findings (4 categories: process violations, Claude mistakes, missing rules, process gaps)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title)

---

## Modes

Learn auto-detects its context:

| Mode | When | Sources | Commits | Settings audit |
|------|------|---------|---------|----------------|
| Phase 5 | State file with Code Review complete | All 3 (CLAUDE.md, context, state/plan) | `/flow-commit --auto` | Yes |
| Maintainer | No state file, `flow-phases.json` exists | 2 (CLAUDE.md, context) | `/flow-commit --auto` | Yes |
| Standalone | No state file, no `flow-phases.json` | 2 (CLAUDE.md, context) | None | Yes |

All three modes route learnings to 2 repo-local destinations: Project
CLAUDE.md and project rules. Both are committed to the repo.

Standalone mode lets any project use `/flow-learn` without a FLOW
feature in progress — just review the current session and apply
learnings.

---

## Mode

Mode is configurable via `.flow.json` (default: auto). In auto mode, permission promotions (Maintainer) are applied automatically and the phase transition advances to Complete without asking.

---

## Gates

- **Phase 5**: Phase 4: Code Review must be complete
- **Maintainer/Standalone**: No gate — runs immediately
- Only CLAUDE.md and `.claude/` files are committed — never application code
