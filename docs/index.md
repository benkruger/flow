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

| Phase | Name | Purpose |
|-------|------|---------|
| [0](phases/phase-0-prepare) | Prepare | Set up the worktree, update gems, establish the PR |
| 1 | Research | *(coming soon)* |
| 2 | Design | *(coming soon)* |
| 3 | Plan | *(coming soon)* |
| 4 | Implement | *(coming soon)* |
| 5 | Test | *(coming soon)* |
| 6 | Review | *(coming soon)* |
| 7 | Ship | *(coming soon)* |

## Installation

```bash
# Add as a Claude Code plugin via git submodule
git submodule add git@github.com:benkruger/ruby-on-rails-claude-ai-process.git .claude/plugins/ror-process
```

## Commands

All commands are namespaced under `ror:`.

| Command | Description |
|---------|-------------|
| `/ror:start <name>` | Begin a new feature — kicks off Phase 0 |
