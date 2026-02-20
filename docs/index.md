---
title: Home
nav_order: 1
---

# ROR Process

An opinionated Ruby on Rails development lifecycle for Claude Code. Every feature — simple or complex — follows the same phases in the same order. No shortcuts.

## Philosophy

- **Always the same phases.** Simple things that seem simple often aren't. The process catches that.
- **Worktree-first.** All work happens in an isolated git worktree. Main is never touched directly.
- **Verify before and after.** `bin/ci` runs at every gate. Green in, green out.
- **Learnings go to CLAUDE.md.** Patterns discovered during a feature get captured as generic Rails conventions, not one-off notes.

## Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | [Start](phases/phase-1-start.md) | `/ror:start` | Set up the worktree, update gems, establish the PR |
| 2 | Research | `/ror:research` | *(coming soon)* |
| 3 | Design | `/ror:design` | *(coming soon)* |
| 4 | Plan | `/ror:plan` | *(coming soon)* |
| 5 | Implement | `/ror:implement` | *(coming soon)* |
| 6 | Test | `/ror:test` | *(coming soon)* |
| 7 | Review | `/ror:review` | *(coming soon)* |
| 8 | Ship | `/ror:ship` | *(coming soon)* |
| 9 | Reflect | `/ror:reflect` | *(coming soon)* |
| 10 | Cleanup | `/ror:cleanup` | *(coming soon)* |

## Installation

```
/plugin marketplace add benkruger/ruby-on-rails-claude-ai-process
/plugin install ror@ruby-on-rails-claude-ai-process
```

## Commands

All commands are namespaced under `ror:`. See the [Skills reference](skills/) for full documentation on each.

| Command | Phase | Description |
|---------|-------|-------------|
| `/ror:start <name>` | 0 | Begin a new feature — sets up worktree, upgrades gems, opens PR |
| `/ror:status` | any | Show current phase, PR link, and what comes next |
| `/ror:commit` | any | Review diff, approve, and commit + push |
