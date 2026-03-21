# Repo-Level Only

All FLOW-produced rules and instructions target the project repo,
never user-level `~/.claude/` paths.

- Rules go to `<project>/.claude/rules/<topic>.md`
- Instructions go to `<project>/CLAUDE.md`
- Reading user-level files (`~/.claude/rules/`, `~/.claude/CLAUDE.md`)
  is fine — writing to them is not
- This applies to every FLOW phase: Plan, Code, Code Review, Learn
