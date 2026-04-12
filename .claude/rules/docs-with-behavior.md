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
- Changed state field ranges, totals, or display names →
  `docs/reference/flow-state-schema.md` (field descriptions
  include hardcoded values like step ranges and totals that
  must match the Rust constants)
- Changed what a skill passes to a sub-agent → the agent's
  `## Input` section in `agents/<name>.md`
- New field, line, or widget in a formatter's output → the
  user-facing SKILL.md that describes the formatter's panel. The
  mapping is explicit: `src/format_status.rs` is described by
  `skills/flow-status/SKILL.md`, `src/format_complete_summary.rs`
  is described by `skills/flow-complete/SKILL.md`, and so on.
  Every conditional line or field shown by the formatter must be
  listed in that SKILL.md's Panel Fields / Output section so a
  future session reading the skill knows what the panel can
  contain.

## Agent Input Section Sync

Agent `## Input` sections are contracts with the model about what
data is available. When a skill changes what artifacts it passes to
a sub-agent (e.g. switching from full diff to substantive diff),
update the agent's Input section in the same commit. Stale Input
sections mislead the agent about available context and produce
incorrect reasoning. CI cannot enforce this (agent Input sections
are prose), so the Plan phase must enumerate all affected agent
files when a skill modifies agent invocations.

## Multi-Task Plans

When a plan splits a behavior change and its documentation update
across separate tasks, the Plan phase should mark them as an atomic
group — or combine them into a single task. The "same commit" rule
means the behavior change and its documentation must land together.
Separate commits within the same PR are not sufficient: if the PR
is reviewed commit-by-commit, the intermediate state shows stale
documentation.

## Scope Enumeration (Rename Side)

This section covers the **rename side** of enumeration — when
fixing drift caused by a renamed or removed identifier. For the
sibling rule covering **coverage claims** — plan prose that
asserts a guard applies universally to a code family without
naming its members — see `.claude/rules/scope-enumeration.md`.
The two rules are complementary: the rename-side grep finds every
file that still mentions the old identifier, while the
coverage-side scanner catches universal-quantifier claims that
lack a named sibling list.

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

When adding a NEW concept (field, panel line, widget, configuration
axis), scope enumeration runs the other direction: there is no "old
identifier" to grep for, so the Plan phase must trace every consumer
of the module being changed. For a formatter module, that means
every SKILL.md that invokes `format-status` or `format-complete-summary`
in a bash block. For a state field, that means `flow-state-schema.md`,
every SKILL.md that reads the field in a bash block, and every agent
`## Input` section that may reference it.

## How to Apply

During the Code phase, when a task modifies a skill SKILL.md or
adds a new `bin/flow` subcommand, check whether any doc file
describes the old behavior. If so, update it in the same task —
do not defer to Code Review or Learn.

During Code Review triage, every documentation finding caused by
the PR's own changes is fixed in the same PR. The Code Review rule
(`.claude/rules/code-review-scope.md`) removes the filing path
entirely — documentation drift introduced by the PR's changes is
a Real finding that gets fixed in Step 4.
