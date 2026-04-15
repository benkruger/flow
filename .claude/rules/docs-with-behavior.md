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
- **New permanent on-main artifact → `CLAUDE.md` "Key Files"
  section.** "Permanent" here means a file that lives on main
  (not `.flow-states/`, not `.flow-issue-body`, not anything
  under `/tmp/`, and not gitignored). Future-session readers
  rely on Key Files as their index to the repository surface
  area; a new permanent artifact that is absent from Key Files
  is effectively invisible until a later PR rediscovers it.
- **Changed type signatures or module architecture → the module-
  level doc comment and every affected item's doc comment in the
  same source file.** This is where PR #1054 missed: splitting
  `FlowPaths` into two types changed the architecture of
  `src/flow_paths.rs`, but the module doc still described the
  single-type model until Code Review caught the drift.
  Source-local doc comments are documentation too — they bind
  the type to its purpose for future readers who arrive via
  grep or rustdoc rather than through the module's external docs.

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

### Named Tests After Refactor

When a plan names specific test functions and a refactor lands
that appears to make those tests redundant (e.g. a shared helper
now owns the logic), the named tests are still required. Add
them — driven through the refactored callsite via a test seam
that accepts an injectable `Command` (or equivalent) — so the
caller-level assertion that the delegation returns the expected
value on each error class is preserved. Coverage waivers are
forbidden; redundant-looking tests are not redundant in practice
because they assert the delegation contract independently of the
helper's internal behavior.

A plan that names tests and a PR that does not add them is a
Code Review finding — the reviewer agent correctly flags "plan
said add X but X is not there."

The rule applies equally to documentation tasks: if a plan task
names a doc update that becomes redundant after another task
supersedes it, the doc update is still required. Reword it to
describe the post-refactor state.

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

During the Plan phase, when the Exploration lists source files
that will be modified, open each file and note every module-level
doc comment and every public item's doc comment. If the planned
change alters the described behavior, add a task — or extend an
existing task — to update those doc comments in the same commit
as the code change. Do not leave source-local doc updates to Code
Review.

During the Code phase, when a task modifies a skill SKILL.md or
adds a new `bin/flow` subcommand, check whether any doc file
describes the old behavior. If so, update it in the same task —
do not defer to Code Review or Learn.

During Code Review triage, every documentation finding caused by
the PR's own changes is fixed in the same PR. The Code Review rule
(`.claude/rules/code-review-scope.md`) removes the filing path
entirely — documentation drift introduced by the PR's changes is
a Real finding that gets fixed in Step 4.
