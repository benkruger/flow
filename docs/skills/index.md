---
title: Skills
nav_order: 3
---

# Skills

Skills are the building blocks of the FLOW workflow. Some are tied to a specific phase and invoked automatically as part of that phase. Others are utility skills available at any point.

All skills are namespaced under `flow:` and announce themselves clearly when they start and finish.

---

## Phase Skills

These skills correspond directly to a workflow phase. Each one starts and ends with a banner so you always know where you are.

| Skill | Phase | Description |
|-------|-------|-------------|
| [`/flow:flow-start`](flow-start.md) | 1 — Start | Create the worktree, upgrade dependencies, open the PR |
| [`/flow:flow-plan`](flow-plan.md) | 2 — Plan | Explore codebase, design approach, produce ordered tasks via plan mode |
| [`/flow:flow-code`](flow-code.md) | 3 — Code | TDD task by task, diff review, `bin/flow ci` gate before each commit |
| [`/flow:flow-simplify`](flow-simplify.md) | 4 — Simplify | Refactor for clarity via `/simplify`, auto-commit accepted changes |
| [`/flow:flow-review`](flow-review.md) | 5 — Review | Delegate to `/review`, fix every finding |
| [`/flow:flow-security`](flow-security.md) | 6 — Security | Delegate to `/security-review`, fix every finding |
| [`/flow:flow-learning`](flow-learning.md) | 7 — Learning | Extract learnings, update CLAUDE.md, note plugin gaps |
| [`/flow:flow-cleanup`](flow-cleanup.md) | 8 — Cleanup | Remove worktree and delete state file — final phase |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/flow:flow-init`](flow-init.md) | One-time setup — configure permissions and git excludes |
| [`/flow:flow-commit`](flow-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/flow:flow-status`](flow-status.md) | Show current phase, PR link, phase checklist, and what comes next |
| [`/flow:flow-continue`](flow-continue.md) | Resume current feature — re-asks last transition question or rebuilds from state |
| [`/flow:flow-note`](flow-note.md) | Capture a correction or learning — invoked automatically on corrections |
| [`/flow:flow-abort`](flow-abort.md) | Abandon the current feature — close PR, delete branch, remove worktree |
| [`/flow:flow-config`](flow-config.md) | Display current configuration — version, framework, per-skill autonomy |
| [`/flow:flow-local-permission`](flow-local-permission.md) | Promote permissions from settings.local.json into settings.json |
