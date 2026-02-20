---
title: Home
nav_order: 1
---

# SDLC Process

An opinionated Ruby on Rails development lifecycle for Claude Code. Every feature — simple or complex — follows the same phases in the same order. No shortcuts.

## Philosophy

- **Always the same phases.** Simple things that seem simple often aren't. The process catches that.
- **Worktree-first.** All work happens in an isolated git worktree. Main is never touched directly.
- **Verify before and after.** `bin/ci` runs at every gate. Green in, green out.
- **Learnings go to CLAUDE.md.** Patterns discovered during a feature get captured as generic Rails conventions, not one-off notes.

## Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | [Start](phases/phase-1-start.md) | `/sdlc:start` | Set up the worktree, update gems, establish the PR |
| 2 | [Research](phases/phase-2-research.md) | `/sdlc:research` | Explore codebase, ask clarifying questions, document findings |
| 3 | Design | `/sdlc:design` | *(coming soon)* |
| 4 | Plan | `/sdlc:plan` | *(coming soon)* |
| 5 | Implement | `/sdlc:implement` | *(coming soon)* |
| 6 | Test | `/sdlc:test` | *(coming soon)* |
| 7 | Review | `/sdlc:review` | *(coming soon)* |
| 8 | Ship | `/sdlc:ship` | *(coming soon)* |
| 9 | Reflect | `/sdlc:reflect` | *(coming soon)* |
| 10 | [Cleanup](phases/phase-10-cleanup.md) | `/sdlc:cleanup` | Remove worktree and delete state file |

## Installation

```
/plugin marketplace add benkruger/ruby-on-rails-claude-ai-process
/plugin install sdlc@ruby-on-rails-claude-ai-process
```

## Commands

All commands are namespaced under `sdlc:`. See the [Skills reference](skills/) for full documentation on each.

| Command | Phase | Description |
|---------|-------|-------------|
| `/sdlc:start <name>` | 0 | Begin a new feature — sets up worktree, upgrades gems, opens PR |
| `/sdlc:resume` | any | Resume current feature — re-asks last transition question mid-session, or rebuilds from state on new session |
| `/sdlc:status` | any | Show current phase, PR link, and what comes next |
| `/sdlc:commit` | any | Review diff, approve, and commit + push |
