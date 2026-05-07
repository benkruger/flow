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
  same source file.** Splitting one type into two changes the
  module's architecture; the module doc and every affected item's
  doc comment must be updated in the same source file in the same
  commit. Source-local doc comments are documentation too — they
  bind the type to its purpose for future readers who arrive via
  grep or rustdoc rather than through the module's external docs.

## Agent Input Section Sync

Agent `## Input` sections are contracts with the model about what
data is available. When a skill changes what artifacts it passes to
a sub-agent (e.g. switching from full diff to substantive diff),
update the agent's Input section in the same commit. Stale Input
sections mislead the agent about available context and produce
incorrect reasoning.

**Plan-phase enumeration requirement.** When a plan task modifies
a skill that invokes one or more sub-agents, the plan's Exploration
table MUST enumerate every affected agent file by path — not "the
four review agents" or "the agents that take the diff," but each
one by name (e.g. `agents/reviewer.md`, `agents/pre-mortem.md`,
`agents/adversarial.md`, `agents/documentation.md`). The
enumeration is the same discipline as
`.claude/rules/scope-enumeration.md` "Vocabulary": a universal
quantifier on a code family must carry a named list. The Code
Review reviewer agent cross-checks plan Exploration against the
agents whose Input sections appear in the staged diff and flags
any agent that was modified but not enumerated as a Real finding.

**Code-phase atomicity.** The skill's invocation change and every
listed agent's Input section update must land in the same commit
per `.claude/rules/plan-commit-atomicity.md`. A plan that splits
the skill update from the agent update across commits is a
Plan-phase gap that must be marked as an atomic group, or merged
into a single task.

## Feature-Configurable Prose Generalization

When a code change introduces support for a configurable parameter
(integration branch, project root, default mode, default channel,
etc.) where prior code hardcoded a single value, every prose
surface that mentions the old hardcoded value must be generalized
in the same PR. Skipping the generalization produces self-referential
drift: the code accepts the configurable input but the docs still
tell users the only valid value is the original hardcoded one.

**The trigger.** A plan task that adds a parameter, state field,
or configuration axis where prior code hardcoded the value. Symptom
language in the plan: "now reads X from Y," "previously hardcoded,"
"plumb through," "honor the configured value."

**The enumeration.** Grep the entire prose corpus for the old
hardcoded value before Code phase begins:

```text
grep -r "<old-value>" CLAUDE.md skills/ docs/ README.md .claude/rules/ agents/
```

Every matching file is in-scope. Group findings by classification:

- **Universal prose** (rule applies to every project) — generalize
  to `<configurable-name>` placeholder or paraphrase that does not
  name a specific value. Example: "Start-Gate CI on Main" →
  "Start-Gate CI on the Base Branch" because the rule applies to
  every repo regardless of whether its trunk is `main`,
  `staging`, `develop`, etc.
- **Self-referential prose** (rule describes THIS repo, where the
  hardcoded value happens to be the only valid value) — leave
  alone. Example: a CLAUDE.md sentence that describes the FLOW
  repo's own release path and references `main` because FLOW's
  own trunk is `main`.

The distinguishing test: would the prose still be correct if it
were applied to a target project where the configurable parameter
holds a different value? If yes → universal → generalize. If no →
self-referential → leave alone.

**Plan-phase task template.** A plan that introduces a
configurable parameter must include a "Generalize universal prose"
task with these subtasks:

1. Enumerate prose corpus matches via the grep above.
2. Classify each match as universal or self-referential.
3. List universal matches in the Exploration table with
   "generalize to `<placeholder>`" notes.
4. Mark the universal-prose task as atomic with the
   implementation task per
   `.claude/rules/plan-commit-atomicity.md` so the prose lands
   in the same PR as the code.

**Code-phase verification.** Before committing the
implementation, re-run the grep to confirm no universal-prose
matches remain. Self-referential matches are expected to remain;
add a one-line note in the commit message body listing the
self-referential paths as deliberately preserved.

## Multi-Task Plans

When a plan splits a behavior change and its documentation update
across separate tasks, the Plan phase should mark them as an atomic
group — or combine them into a single task. The "same commit" rule
means the behavior change and its documentation must land together.
Separate commits within the same PR are not sufficient: if the PR
is reviewed commit-by-commit, the intermediate state shows stale
documentation.

For the sibling Plan-phase discipline covering **new branches
introduced by extraction refactors** — the plan must enumerate each
extracted helper's branches with a testability classification
before Code phase begins — see
`.claude/rules/extract-helper-refactor.md`.

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

When renaming a command, replacing a subcommand, or fixing
documentation drift, grep all files for **every** old identifier
the change removes or renames — not just the obvious one. A
single PR often touches multiple identifiers simultaneously:
removing a feature deletes its symbol name AND any prose phrase
that named the behavior; renumbering a numbered concept (e.g.
"Layer 8" → "Layer 9") leaves stale references to the description
("file-read commands") that named the OLD layer's purpose. The
Plan phase must enumerate every identifier the change touches
before running the grep sweep.

The discipline is two-step:

1. **Identifier inventory.** Before writing the plan, list every
   string the change removes or renames. For a feature removal,
   that includes: the symbol name (e.g. `FILE_READ_COMMANDS`),
   the prose name (e.g. "file-read commands"), the layer
   number/identifier (e.g. "Layer 8"), and any other mnemonic
   the prose corpus uses for the same concept. For a rename,
   that includes: the old name AND the OLD prose phrasing that
   referenced it.
2. **Per-identifier grep.** Run a separate grep for each
   identifier in the inventory:

   ```text
   grep -r "<identifier-1>" docs/ skills/ tests/ CLAUDE.md .claude/rules/
   grep -r "<identifier-2>" docs/ skills/ tests/ CLAUDE.md .claude/rules/
   ```

Every matching file is in-scope regardless of what the issue body
or plan names. This applies both reactively (fixing drift) and
proactively (renaming a command as part of a feature). The Plan
phase must enumerate the full scope, not echo the issue's file
list.

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
