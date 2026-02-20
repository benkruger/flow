---
title: Skills
nav_order: 3
---

# Skills

Skills are the building blocks of the SDLC workflow. Some are tied to a specific phase and invoked automatically as part of that phase. Others are utility skills available at any point.

All skills are namespaced under `sdlc:` and announce themselves clearly when they start and finish.

---

## Phase Skills

These skills correspond directly to a workflow phase. Each one starts and ends with a banner so you always know where you are.

| Skill | Phase | Description |
|-------|-------|-------------|
| [`/sdlc:start`](sdlc-start.md) | 1 — Start | Create the worktree, upgrade gems, open the PR, configure permissions |
| [`/sdlc:research`](sdlc-research.md) | 2 — Research | Explore codebase, ask clarifying questions, document findings |
| `/sdlc:design` | 3 — Design | *(coming soon)* |
| `/sdlc:plan` | 4 — Plan | *(coming soon)* |
| `/sdlc:implement` | 5 — Implement | *(coming soon)* |
| `/sdlc:test` | 6 — Test | *(coming soon)* |
| `/sdlc:review` | 7 — Review | *(coming soon)* |
| `/sdlc:ship` | 8 — Ship | *(coming soon)* |
| `/sdlc:reflect` | 9 — Reflect | *(coming soon)* |
| `/sdlc:cleanup` | 10 — Cleanup | *(coming soon)* |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/sdlc:commit`](sdlc-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/sdlc:status`](sdlc-status.md) | Show current phase, PR link, phase checklist, and what comes next |
