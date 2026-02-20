---
title: Skills
nav_order: 3
---

# Skills

Skills are the building blocks of the ROR workflow. Some are tied to a specific phase and invoked automatically as part of that phase. Others are utility skills available at any point.

All skills are namespaced under `ror:` and announce themselves clearly when they start and finish.

---

## Phase Skills

These skills correspond directly to a workflow phase. Each one starts and ends with a banner so you always know where you are.

| Skill | Phase | Description |
|-------|-------|-------------|
| [`/ror:start`](ror-start.md) | 1 — Start | Create the worktree, upgrade gems, open the PR, configure permissions |
| [`/ror:research`](ror-research.md) | 2 — Research | Explore codebase, ask clarifying questions, document findings |
| `/ror:design` | 3 — Design | *(coming soon)* |
| `/ror:plan` | 4 — Plan | *(coming soon)* |
| `/ror:implement` | 5 — Implement | *(coming soon)* |
| `/ror:test` | 6 — Test | *(coming soon)* |
| `/ror:review` | 7 — Review | *(coming soon)* |
| `/ror:ship` | 8 — Ship | *(coming soon)* |
| `/ror:reflect` | 9 — Reflect | *(coming soon)* |
| `/ror:cleanup` | 10 — Cleanup | *(coming soon)* |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/ror:commit`](ror-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/ror:status`](ror-status.md) | Show current phase, PR link, phase checklist, and what comes next |
