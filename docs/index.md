---
title: Home
nav_order: 1
---

# FLOW Process

An opinionated Ruby on Rails development lifecycle for Claude Code. Every feature — simple or complex — follows the same phases in the same order. No shortcuts.

## Philosophy

- **Always the same phases.** Simple things that seem simple often aren't. The process catches that.
- **Worktree-first.** All work happens in an isolated git worktree. Main is never touched directly.
- **Verify before and after.** `bin/ci` runs at every gate. Green in, green out.
- **Learnings go to CLAUDE.md.** Patterns discovered during a feature get captured as generic Rails conventions, not one-off notes.

## Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | [Start](phases/phase-1-start.md) | `/flow:start` | Set up the worktree, update gems, establish the PR |
| 2 | [Research](phases/phase-2-research.md) | `/flow:research` | Explore codebase, ask clarifying questions, document findings |
| 3 | [Design](phases/phase-3-design.md) | `/flow:design` | Propose 2-3 alternatives, get approval before any code |
| 4 | [Plan](phases/phase-4-plan.md) | `/flow:plan` | Break design into ordered TDD tasks, section by section |
| 5 | Implement | `/flow:implement` | *(coming soon)* |
| 6 | Review | `/flow:review` | *(coming soon)* |
| 7 | Ship | `/flow:ship` | *(coming soon)* |
| 8 | Reflect | `/flow:reflect` | *(coming soon)* |
| 9 | [Cleanup](phases/phase-9-cleanup.md) | `/flow:cleanup` | Remove worktree and delete state file |

## Installation

```
/plugin marketplace add benkruger/ruby-on-rails-claude-ai-process
/plugin install flow@ruby-on-rails-claude-ai-process
```

## Commands

All commands are namespaced under `flow:`. See the [Skills reference](skills/) for full documentation on each.

| Command | Phase | Description |
|---------|-------|-------------|
| `/flow:start <name>` | 0 | Begin a new feature — sets up worktree, upgrades gems, opens PR |
| `/flow:resume` | any | Resume current feature — re-asks last transition question mid-session, or rebuilds from state on new session |
| `/flow:status` | any | Show current phase, PR link, and what comes next |
| `/flow:commit` | any | Review diff, approve, and commit + push |
