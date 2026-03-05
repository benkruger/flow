---
title: /flow:reflect
nav_order: 9
parent: Skills
---

# /flow:reflect

**Phase:** 7 — Reflect

**Usage:** `/flow:reflect`

Autonomously synthesises what went wrong from four sources, routes each
learning to its correct permanent home, files GitHub issues for plugin
improvements, and presents a comprehensive report. Runs before the PR
merges.

---

## Sources

| Source | What | Survives compaction? |
|--------|------|---------------------|
| State file | Visit counts, timing, notes array | Yes |
| `/flow:note` captures | Corrections logged automatically | Yes |
| Conversation context | Session back-and-forth | Only if not compacted |
| Worktree auto-memory | Patterns and observations from feature work | Yes |

---

## Outputs

Learnings are routed autonomously to one of 5 destinations:

| # | Destination | Path |
|---|-------------|------|
| 1 | Global CLAUDE.md | `~/.claude/CLAUDE.md` |
| 2 | Project CLAUDE.md | `CLAUDE.md` in worktree |
| 3 | Global rules | `~/.claude/rules/<topic>.md` |
| 4 | Project rules | `.claude/rules/<topic>.md` in worktree |
| 5 | Project memory | `~/.claude/projects/<repo-root>/memory/MEMORY.md` |

Destinations 1, 3, 5 are user-private (direct edits, not committed).
Destinations 2, 4 are committed to the feature branch via `/flow:commit --auto`.

**Plugin improvement notes** — filed as GitHub issues:

- One issue per process gap on the plugin repo, labeled `reflect`
- Issue body describes the gap generically (no user project details)

**Report** — presented after all changes are applied:

- Findings (5 categories: process violations, Claude mistakes, missing rules, process gaps, worth preserving)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title)

---

## Modes

Reflect auto-detects its context:

| Mode | When | Sources | Commits | Settings audit |
|------|------|---------|---------|----------------|
| Phase 7 | State file with Security complete | All 4 | `/flow:commit --auto` | No |
| Maintainer | No state file, `flow-phases.json` exists | A + B | `/flow:commit --auto` | Yes |
| Standalone | No state file, no `flow-phases.json` | A + B | None | No |

Standalone mode lets any project use `/flow:reflect` without a FLOW
feature in progress — just reflect on the current session and apply
learnings.

---

## Gates

- **Phase 7**: Phase 6: Security must be complete
- **Maintainer/Standalone**: No gate — runs immediately
- Only CLAUDE.md and `.claude/` files are committed — never application code
