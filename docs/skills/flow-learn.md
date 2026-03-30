---
title: /flow-learn
nav_order: 9
parent: Skills
---

# /flow-learn

**Phase:** 5 — Learn

**Usage:** `/flow-learn`, `/flow-learn --auto`, `/flow-learn --manual`, or `/flow-learn --continue-step`

Autonomously synthesises what went wrong from four sources (two inline
plus two context-isolated agents in Phase 5), routes each learning to its
correct permanent home, promotes session permissions from
`settings.local.json` into `settings.json`, files GitHub issues for
plugin improvements, and presents a comprehensive report. Runs before the
PR merges.

---

## Sources

| Source | What | Survives compaction? |
|--------|------|---------------------|
| CLAUDE.md rules | Project rules and conventions that should have been followed | Yes |
| Learn-analyst agent | Categorized findings from cognitively isolated artifact analysis (Phase 5 only) | N/A (agent output) |
| Conversation context | Session back-and-forth (Maintainer and Standalone only) | Only if not compacted |
| State file and plan data | Visit counts, timing, notes, plan risks (Phase 5 only) | Yes |
| Onboarding agent | Confusion report from a context-isolated newcomer perspective (Phase 5 only) | N/A (agent output) |

---

## Outputs

Learnings are routed autonomously based on destination:

| # | Destination | Path | Method |
|---|-------------|------|--------|
| 1 | Project CLAUDE.md | `CLAUDE.md` in worktree | `bin/flow write-rule` |
| 2 | `.claude/rules/` | `.claude/rules/<topic>.md` in worktree | `bin/flow write-rule` |

Both CLAUDE.md and `.claude/rules/` edits are committed to the feature branch
via `/flow-commit --auto`. All edits target the project repo — never
user-level `~/.claude/` paths.

**Permission promotion** — session permissions accumulated in
`.claude/settings.local.json` are merged into `.claude/settings.json`
via `bin/flow promote-permissions`. The local file is deleted after
merging. Runs in all three modes.

**GitHub issues** — filed during Learn:

- **Flow** — FLOW process gaps, on the plugin repo (`benkruger/flow`)

All filed issues are recorded in the state file via `bin/flow add-issue`.

**Report** — presented after all changes are applied:

- Findings (4 categories: process violations, mistakes, missing rules, process gaps — from learn-analyst and onboarding agents in Phase 5, from conversation review in other modes)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title)

---

## Modes

Learn auto-detects its context:

| Mode | When | Sources | Commits | GitHub issues |
|------|------|---------|---------|---------------|
| Phase 5 | State file with Code Review complete | All (CLAUDE.md, learn-analyst agent, state/plan, onboarding agent) | `/flow-commit --auto` | Yes |
| Maintainer | No state file, `flow-phases.json` exists | 2 (CLAUDE.md, conversation context) | `/flow-commit --auto` | No |
| Standalone | No state file, no `flow-phases.json` | 2 (CLAUDE.md, conversation context) | None | No |

All three modes route both CLAUDE.md and `.claude/rules/` changes directly
to the project repo. Both destinations are written via `bin/flow write-rule`
and committed.

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
