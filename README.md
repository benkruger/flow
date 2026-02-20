# FLOW Process

An opinionated Ruby on Rails development lifecycle plugin for [Claude Code](https://docs.anthropic.com/en/docs/claude-code).

Every feature — simple or complex — follows the same 8 phases in the same order. No shortcuts.

**Documentation:** [benkruger.github.io/ruby-on-rails-claude-ai-process](https://benkruger.github.io/ruby-on-rails-claude-ai-process)

## Why

Claude Code is powerful but undisciplined. Without structure, it skips research, writes code before understanding the codebase, and misses Rails-specific gotchas like callbacks overwriting your values or soft deletes hiding records.

FLOW forces a consistent process:
- **Research before design.** Read the full class hierarchy. Find the callbacks. Check `test/support/` for existing helpers.
- **Design before code.** Propose 2-3 alternatives. Get approval. Never start coding without a plan.
- **TDD always.** Test fails first. Then implementation. Then bin/ci. Every task.
- **Learnings survive.** Corrections are captured to the state file automatically. Reflect writes them back to CLAUDE.md as reusable patterns.

## Installation

In any Claude Code session:

```
/plugin marketplace add benkruger/ruby-on-rails-claude-ai-process
/plugin install flow@flow-marketplace
```

Then start a feature:

```
/flow:start app payment webhooks
```

## The 8 Phases

```
Start → Research → Design → Plan → Code → Review → Reflect → Cleanup
```

| Phase | Command | What it does |
|-------|---------|-------------|
| 1: Start | `/flow:start <name>` | Worktree, bundle update, PR, permissions, baseline bin/ci |
| 2: Research | `/flow:research` | Read affected code, ask clarifying questions, document findings |
| 3: Design | `/flow:design` | Propose 2-3 alternatives, get approval before any code |
| 4: Plan | `/flow:plan` | Break design into ordered TDD tasks, approve section by section |
| 5: Code | `/flow:code` | TDD per task, diff review, bin/ci gate, commit per task |
| 6: Review | `/flow:review` | Design alignment, risk coverage, Rails anti-pattern check |
| 7: Reflect | `/flow:reflect` | Extract learnings, update CLAUDE.md, note plugin improvements |
| 8: Cleanup | `/flow:cleanup` | Remove worktree, delete state file |

## Utility Commands

Available at any point in the workflow:

| Command | What it does |
|---------|-------------|
| `/flow:commit` | Review diff, approve/deny, pull before push, commit |
| `/flow:status` | Show current phase, PR link, timing, next step |
| `/flow:resume` | Resume mid-session or rebuild from state on new session |
| `/flow:note` | Capture corrections — auto-invoked when Claude is wrong |

## How It Works

- **State file** per feature at `.claude/flow-states/<branch>.json` — tracks phase status, timing, research findings, design decisions, plan tasks, and captured notes
- **SessionStart hook** detects in-progress features and resumes automatically
- **Phase guards** prevent skipping ahead — each phase checks the previous one is complete
- **Back navigation** lets you return to earlier phases when something is wrong
- **Multiple features** can run simultaneously in separate worktrees

## What It Enforces

- Worktree isolation — main is never touched directly
- bin/ci green before every commit and every phase transition
- TDD — test must fail before implementation code is written
- No `git rebase` — merge only
- No disabling RuboCop — fix the code, not the cop
- 100% test coverage maintained
- Commit messages follow imperative verb + tl;dr + file breakdown format

## Updating

```
/plugin update flow@flow-marketplace
```

## License

[MIT](LICENSE)