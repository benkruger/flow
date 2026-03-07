---
title: /flow:learning
nav_order: 9
parent: Skills
---

# /flow:learning

**Phase:** 7 — Learning

**Usage:** `/flow:learning`, `/flow:learning --auto`, or `/flow:learning --manual`

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
| State file and plan data | Visit counts, timing, notes, plan risks (Phase 7 only) | Yes |

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

- One issue per process gap on the plugin repo, labeled `learning`
- Issue body describes the gap generically (no user project details)

**Report** — presented after all changes are applied:

- Findings (4 categories: process violations, Claude mistakes, missing rules, process gaps)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title)

---

## Modes

Learning auto-detects its context:

| Mode | When | Sources | Commits | Settings audit |
|------|------|---------|---------|----------------|
| Phase 7 | State file with Security complete | All 3 (CLAUDE.md, context, state/plan) | `/flow:commit --auto` | No |
| Maintainer | No state file, `flow-phases.json` exists | 2 (CLAUDE.md, context) | `/flow:commit --auto` | Yes |
| Standalone | No state file, no `flow-phases.json` | 2 (CLAUDE.md, context) | None | No |

Standalone mode lets any project use `/flow:learning` without a FLOW
feature in progress — just review the current session and apply
learnings.

---

## Mode

Mode is configurable via `.flow.json` (default: auto). In auto mode, permission promotions (Maintainer) are applied automatically and the phase transition advances to Cleanup without asking.

---

## Gates

- **Phase 7**: Phase 6: Security must be complete
- **Maintainer/Standalone**: No gate — runs immediately
- Only CLAUDE.md and `.claude/` files are committed — never application code
