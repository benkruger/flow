---
title: /flow-sweep
nav_order: 16
parent: Skills
---

# /flow-sweep

**Phase:** Any

**Usage:** `/flow-sweep`, `/flow-sweep --status`, `/flow-sweep --cleanup`

Batch-process GitHub issues in parallel. Spawns autonomous worker agents — each gets its own worktree, implements a fix, runs tests, and opens a pull request. Use `--status` to check progress anytime.

---

## What It Does

1. Fetches open issues via `gh issue list`
2. Presents issues for selection (all, specific numbers, or cancel)
3. Spawns one `issue-worker` agent per issue, each in its own worktree
4. Workers autonomously: explore, plan, code, test, commit, and open a PR
5. Tracks progress in `.flow-states/sweep.json`
6. Displays a dashboard with issue status and PR links

---

## Modes

| Flag | Behavior |
|------|----------|
| *(none)* | Fetch issues, select, spawn workers, show dashboard |
| `--status` | Show current sweep dashboard |
| `--cleanup` | Remove worktrees for completed/failed issues |

---

## Concurrency

Workers run in parallel up to a configurable limit (default: 3). Issues beyond the limit are queued and dispatched as workers complete.

---

## Gates

- Workers create PRs — never modify issues directly
- State mutations always go through `bin/flow sweep-update`
- Respects concurrency limit
