# Research the Target Project

When running a FLOW phase in a target project, all codebase
exploration (Read, Grep, Glob, Agent/Explore) must target the
project you are working in — the worktree or project root.

Never research the FLOW plugin source to understand the target
project's code. The plugin is infrastructure; the target project
is what you are building.

## Sub-Agent Scoping

When launching sub-agents during any FLOW phase — including
agents spawned by third-party plugins like code-review:code-review
— scope all file searches to the repo/worktree. Never instruct
sub-agents to search `~/.claude/`, `/tmp/`, or paths outside the
project directory. Pass the repo root explicitly in the agent
prompt when the agent needs to discover files (e.g. CLAUDE.md).

## Common Mistakes

- Reading `~/.claude/plugins/cache/` files to understand how
  the project works
- Grepping the plugin repo instead of the worktree
- Exploring FLOW skills or lib scripts when the user asks
  "how does X work" about their project code
- Using FLOW's own test patterns as a reference for the
  target project's test conventions
- Launching sub-agents that search `~/.claude/` or `/tmp/`
  for project files like CLAUDE.md

## When Plugin Research Is Valid

- Debugging a FLOW skill or hook that is misbehaving
- The user explicitly asks about FLOW internals
- Developing FLOW itself (working directory is the plugin repo)