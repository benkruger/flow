# Scope Enumeration for Universal-Coverage Claims

When a plan task, CLAUDE.md section, or rule file uses a universal
quantifier applied to a function family or CLI category, the prose
must be accompanied by a named list of the concrete siblings the
claim covers. Without the list, the Code phase has no checklist and
downstream reviewers consistently catch uncovered siblings.

## Why

A universal-quantifier claim without a named list is an invisible
checklist. Two observed incidents (#1033):

- A plan for `cwd_scope::enforce` said the guard applied to the
  state-mutating subcommand family. Code landed the guard in the
  five CI-tier tool runners (`ci`, `build`, `lint`, `format`,
  `test`) and missed the five state mutators (`add-finding`,
  `phase-finalize`, `phase-enter`, `phase-transition`,
  `set-timestamp`). Code Review caught the gap; the fix touched
  another five files.
- A `FLOW_CI_RUNNING` recursion guard landed in `ci.rs::run()` but
  missed `build.rs`, `lint.rs`, `format_check.rs`, and
  `test_runner.rs`.

Both plans used a universal phrase without enumerating. Both ended
with avoidable Code Review back-and-forth.

## The Rule

For every prose sentence that combines a universal quantifier with
a code-family noun, include a named list adjacent to the phrase:

- an inline parenthetical with backtick-quoted identifiers, OR
- a bullet list with backtick-quoted identifiers immediately before
  or after the sentence, OR
- a table row that enumerates the family

The list must name every concrete sibling, not a representative
sample. The vocabulary is closed and curated — see
`src/scope_enumeration.rs` for the current trigger noun set.

## How to Enumerate

For FLOW's two known guard families, the enumeration lives in
`.claude/rules/rust-patterns.md` under "Guard Universality Across
CLI Entry Points." Copy the list from there.

For any other family, grep `src/main.rs` for `Commands::<VariantName>`
entries that match the family and list every variant by name. If the
family is genuinely open-ended (e.g., "every supported git version"),
use the opt-out comment instead of a forced list.

## Where This Applies

- **Plan files** (`.flow-states/<branch>-plan.md`) — scanned at Plan
  phase completion by `bin/flow plan-check`. Gated in the standard
  path by `skills/flow-plan/SKILL.md` Step 4 and in the pre-planned
  path by `src/plan_extract.rs`.
- **`CLAUDE.md`** — scanned by a contract test in
  `tests/scope_enumeration.rs`.
- **`.claude/rules/*.md`** — same contract test.
- **`skills/**/SKILL.md` and `.claude/skills/**/SKILL.md`** — same
  contract test.
- **Agent prompts and issue bodies** — not mechanically enforced in
  iteration 1; the rule is the primary instrument.

## Opt-Out Comments

Two line-level opt-out comments are recognized by the scanner:

- An `open-ended` opt-out for genuinely unbounded families where no
  finite list can be produced.
- An `imperative` opt-out for instructional phrasing that tells the
  reader to perform an action rather than asserting coverage.

The opt-out applies to the line it sits on and to the next non-blank
line. Choose the correct flavor — a bare opt-out without a reason is
a future-reader hazard.

Example of the imperative form (rendered inside a fenced block so the
example does not itself trigger the scanner):

```markdown
<!-- scope-enumeration: imperative -->
1. Grep for all callers of the function.
```

## Enforcement

Two mechanical enforcers back the rule:

- `bin/flow plan-check` gates Plan phase completion. The standard
  path invokes it from `skills/flow-plan/SKILL.md` Step 4 before
  `phase-transition --action complete`; the pre-decomposed path
  invokes it from `src/plan_extract.rs` before `complete_plan_phase`.
  A non-empty violation list blocks phase completion.
- `tests/scope_enumeration.rs` scans the committed prose corpus
  (`CLAUDE.md`, `.claude/rules/*.md`, `skills/**/SKILL.md`,
  `.claude/skills/**/SKILL.md`) once per CI run. A single new
  unenumerated universal claim fails the build.

Both enforcers share `src/scope_enumeration.rs::scan` so the trigger
vocabulary and the enumeration-present heuristic cannot drift between
the plan-time gate and the prose-drift tripwire.

## Vocabulary Extensibility

The trigger vocabulary is closed by design — novel phrasings that
slip past the scanner are handled by extending the vocabulary in
follow-up commits, mirroring the curated-pattern discipline
documented for the backward-facing comment scanner in
`.claude/rules/comment-quality.md`. The rule file is the primary
instrument; the scanner is the merge-conflict trip-wire. When a
reviewer finds a new phrasing that should have been caught, add it
to `SCOPE_TRIGGER_PATTERN` in `src/scope_enumeration.rs` and note
the addition in the commit message.
