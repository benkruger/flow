---
title: Skills
nav_order: 3
---

# Skills

Skills are the building blocks of the FLOW workflow. Some are tied to a specific phase and invoked automatically as part of that phase. Others are utility skills available at any point.

All skills announce themselves clearly when they start and finish.

---

## Phase Skills

These skills correspond directly to a workflow phase. Each one starts and ends with a banner so you always know where you are.

| Skill | Phase | Description |
|-------|-------|-------------|
| [`/flow-start`](flow-start.md) | 1 — Start | Create the worktree, upgrade dependencies, open the PR |
| [`/flow-plan`](flow-plan.md) | 2 — Plan | Explore codebase, design approach, produce ordered tasks via plan mode |
| [`/flow-code`](flow-code.md) | 3 — Code | TDD task by task, diff review, `bin/flow ci` gate before each commit |
| [`/flow-code-review`](flow-code-review.md) | 4 — Code Review | Four lenses — clarity, correctness, safety, and CLAUDE.md compliance |
| [`/flow-learn`](flow-learn.md) | 5 — Learn | Extract learnings, update CLAUDE.md, note plugin gaps |
| [`/flow-complete`](flow-complete.md) | 6 — Complete | Merge PR, remove worktree, delete state file — final phase |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/flow-prime`](flow-prime.md) | One-time setup — configure permissions and git excludes |
| [`/flow-commit`](flow-commit.md) | Review the full diff, approve or deny, then git add + commit + push |
| [`/flow-status`](flow-status.md) | Show current phase, PR link, phase checklist, and what comes next |
| [`/flow-continue`](flow-continue.md) | Resume current feature — re-asks last transition question or rebuilds from state |
| [`/flow-note`](flow-note.md) | Capture a correction or learning — invoked automatically on corrections |
| [`/flow-abort`](flow-abort.md) | Abandon the current feature — close PR, delete branch, remove worktree |
| [`/flow-config`](flow-config.md) | Display current configuration — version, framework, per-skill autonomy |
| [`/flow-issues`](flow-issues.md) | Fetch open issues, categorize, prioritize, detect batchable issues, and display a dashboard with recommended work order |
| [`/flow-local-permission`](flow-local-permission.md) | Promote permissions from settings.local.json into settings.json |
