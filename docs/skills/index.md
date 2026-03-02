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
| [`/flow:start`](flow-start.md) | 1 — Start | Create the worktree, upgrade dependencies, open the PR |
| [`/flow:plan`](flow-plan.md) | 2 — Plan | Explore codebase, design approach, produce ordered tasks via plan mode |
| [`/flow:code`](flow-code.md) | 3 — Code | TDD task by task, diff review, bin/ci gate before each commit |
| [`/flow:review`](flow-review.md) | 4 — Review | Plan alignment, risk coverage, framework anti-pattern check |
| [`/flow:security`](flow-security.md) | 5 — Security | Scan for security issues in the feature diff |
| [`/flow:reflect`](flow-reflect.md) | 6 — Reflect | Extract learnings, update CLAUDE.md, note plugin gaps |
| [`/flow:cleanup`](flow-cleanup.md) | 7 — Cleanup | Remove worktree and delete state file — final phase |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/flow:init`](flow-init.md) | One-time setup — configure permissions and git excludes |
| [`/flow:commit`](flow-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/flow:status`](flow-status.md) | Show current phase, PR link, phase checklist, and what comes next |
| [`/flow:continue`](flow-continue.md) | Resume current feature — re-asks last transition question or rebuilds from state |
| [`/flow:note`](flow-note.md) | Capture a correction or learning — invoked automatically on corrections |
| [`/flow:abort`](flow-abort.md) | Abandon the current feature — close PR, delete branch, remove worktree |
