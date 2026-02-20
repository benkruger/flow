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
| [`/flow:start`](flow-start.md) | 1 — Start | Create the worktree, upgrade gems, open the PR, configure permissions |
| [`/flow:research`](flow-research.md) | 2 — Research | Explore codebase, ask clarifying questions, document findings |
| [`/flow:design`](flow-design.md) | 3 — Design | Propose 2-3 alternatives, get approval before any code |
| [`/flow:plan`](flow-plan.md) | 4 — Plan | Break design into ordered TDD tasks, section by section |
| `/flow:implement` | 5 — Implement | *(coming soon)* |
| `/flow:review` | 6 — Review | *(coming soon)* |
| `/flow:ship` | 7 — Ship | *(coming soon)* |
| `/flow:reflect` | 8 — Reflect | *(coming soon)* |
| [`/flow:cleanup`](flow-cleanup.md) | 9 — Cleanup | Remove worktree and delete state file — final phase |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/flow:commit`](flow-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/flow:status`](flow-status.md) | Show current phase, PR link, phase checklist, and what comes next |
