# Docs With Behavior

When a change modifies behavior that documentation describes, update
the docs in the same commit — not in a follow-up issue.

Filing an issue for documentation you just made stale is double work:
the next session must re-read the code and re-understand the change
to write the same updates you could write now.

## What Counts

- Changed skill steps or flags → `docs/skills/<name>.md`
- Changed phase behavior → `docs/phases/phase-<N>-<name>.md`
- New CLI subcommand or changed state mutations → `CLAUDE.md`
  architecture sections, `docs/reference/flow-state-schema.md`

## Scope Enumeration

When renaming a command, replacing a subcommand, or fixing
documentation drift, grep all files for the old identifier before
writing the plan:

```text
grep -r "<old-name>" docs/ skills/ tests/ CLAUDE.md .claude/rules/
```

Every matching file is in-scope regardless of what the issue body
or plan names. This applies both reactively (fixing drift) and
proactively (renaming a command as part of a feature). The Plan
phase must enumerate the full scope, not echo the issue's file list.

## How to Apply

During the Code phase, when a task modifies a skill SKILL.md or
adds a new `bin/flow` subcommand, check whether any doc file
describes the old behavior. If so, update it in the same task —
do not defer to Code Review or Learn.

During Code Review triage, documentation findings caused by the
PR's own changes are always in-scope. Never classify them as
"out-of-scope" or file them as issues.
