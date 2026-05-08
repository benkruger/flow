# Scope Enumeration for Universal-Coverage Claims

When a plan task, CLAUDE.md section, or rule file uses a universal
quantifier applied to a function family or CLI category, the prose
must be accompanied by a named list of the concrete siblings the
claim covers. Without the list, the Code phase has no checklist and
downstream reviewers consistently catch uncovered siblings.

## Why

A universal-quantifier claim without a named list is an invisible
checklist. A plan saying "the guard applies to the state-mutating
subcommand family" doesn't tell the Code phase WHICH subcommands
to touch — and the Code phase will land the guard in some siblings
and miss others. Code Review then has to find the misses, costing
a full review cycle on work the Plan phase could have prevented.

## The Rule

For every prose sentence that combines a universal quantifier with
a code-family noun, include a named list adjacent to the phrase:

- an inline parenthetical or colon list with backtick-quoted
  identifiers on the same line, OR
- a bullet list with backtick-quoted identifiers immediately before
  or after the sentence, OR
- a table row that enumerates the family

The list must name every concrete sibling, not a representative
sample.

## Vocabulary

The trigger vocabulary is closed and curated. The scanner in
`src/scope_enumeration.rs` currently flags these noun phrases when
paired with a universal quantifier (`every`, `all`, `each`) and up
to two optional intervening adjectives:

- `subcommand` (and plural `subcommands`)
- `runner`
- `entry point`
- `state mutator` (and hyphenated `state-mutator`)
- `mutator` (bare)
- `CLI variant`, `CLI entry`
- `callsite`
- `caller`
- `dispatch path`
- `handler`

**Intentional gaps.** Bare `command` and bare `module` are NOT in
the vocabulary. Adding them would catch novel phrasings but also
flag pre-existing imperative prose in this tree. The scanner's
curated-closed philosophy prefers to miss a novel phrasing over
introducing mass false positives — the rule file itself is the
primary instrument and the contract test is the drift tripwire.
Unit tests `trigger_rejects_bare_command_intentionally` and
`trigger_rejects_bare_module_intentionally` lock these gaps in so
a future widening is a deliberate decision, not an accident.

When a reviewer finds a novel phrasing that slips past the scanner,
the fix is to add the noun to `SCOPE_TRIGGER_PATTERN` in
`src/scope_enumeration.rs`, add a matching trigger unit test,
update this list, and note the addition in the commit message.

## How to Enumerate

For FLOW's two known guard families, the enumeration lives in
`.claude/rules/rust-patterns.md` under "Guard Universality Across
CLI Entry Points." Copy the list from there.

For any other family, grep `src/main.rs` for `Commands::<VariantName>`
entries that match the family and list every variant by name. If the
family is genuinely open-ended (e.g., "every supported git version"),
use the opt-out comment instead of a forced list.

## Where This Applies

- **Plan files** (`.flow-states/<branch>/plan.md`) — scanned at Plan
  phase completion by `bin/flow plan-check`. Gated in the standard
  path by `skills/flow-plan/SKILL.md` Step 4 and in the pre-planned
  path by `src/plan_extract.rs` extracted and resume paths.
- **`CLAUDE.md`** — scanned by a contract test in
  `tests/scope_enumeration.rs`.
- **`.claude/rules/*.md`** — same contract test.
- **`skills/**/SKILL.md` and `.claude/skills/**/SKILL.md`** — same
  contract test.
- **Agent prompts and issue bodies** — not mechanically enforced;
  the rule is the primary instrument.

## Opt-Out Comments

Two line-level opt-out comments are recognized by the scanner:

- An `open-ended` opt-out for genuinely unbounded families where no
  finite list can be produced.
- An `imperative` opt-out for instructional phrasing that tells the
  reader to perform an action rather than asserting coverage.

The opt-out applies to its own line and to the next non-blank line,
with **at most one blank line separating them**. An opt-out at the
top of a section cannot silence arbitrary triggers further down —
the walk-back is bounded to one blank line.

Example of the imperative form (rendered inside a fenced block so
the example does not itself trigger the scanner):

```markdown
<!-- scope-enumeration: imperative -->
1. Grep for all callers of the function.
```

## Enforcement

Two mechanical enforcers back the rule:

- `bin/flow plan-check` gates Plan phase completion at three
  callsites (the standard path via `skills/flow-plan/SKILL.md`
  Step 4, the pre-decomposed extracted path in `src/plan_extract.rs`,
  and the resume path in the same file). All three callsites share
  `src/scope_enumeration.rs::scan` so the trigger vocabulary and the
  enumeration-present heuristic cannot drift between callsites. A
  non-empty violation list blocks phase completion; editing the plan
  file in place is the only way through.
- `tests/scope_enumeration.rs` runs during every `bin/flow ci`
  invocation and scans the committed prose corpus. A single new
  unenumerated universal claim fails the build automatically.

## Enumeration Heuristic

The scanner accepts three structural patterns as proof of a named
enumeration near a trigger:

- **Inline list after the trigger.** The trigger line itself
  contains at least three backtick-quoted spans after the trigger
  match — catches colon-delimited and parenthetical lists on the
  same line.
- **Forward bullet list.** Within the next eight non-blank lines
  after the trigger, at least one line begins with `-` or `*` AND
  the total backtick count is ≥ 2. Multi-line bullet continuations
  contribute to the total.
- **Backward bullet list.** Symmetric to the forward case, for
  lists that precede the trigger.

Loose backtick counts alone (two unrelated code references in the
same paragraph) do NOT satisfy the heuristic — a real structured
list is required.

## Vocabulary Extensibility

The trigger vocabulary is closed by design — novel phrasings that
slip past the scanner are handled by extending the vocabulary in
follow-up commits, mirroring the curated-pattern discipline
documented for the backward-facing comment scanner in
`.claude/rules/comment-quality.md`. The rule file is the primary
instrument; the scanner is the merge-conflict trip-wire. When a
reviewer finds a new phrasing that should have been caught, add it
to `SCOPE_TRIGGER_PATTERN` in `src/scope_enumeration.rs`, add the
matching trigger unit test, update the Vocabulary section above,
and note the addition in the commit message.

### False-positive sweep before expanding the vocabulary

Before accepting a proposed vocabulary expansion, run a mandatory
false-positive sweep. The protocol is:

1. Add the candidate noun to `SCOPE_TRIGGER_PATTERN` on a scratch
   branch and run `bin/flow ci --test -- scope_enumeration`. The
   contract test in `tests/scope_enumeration.rs` will report every
   pre-existing prose line that now triggers.
2. Count the new violations. If the count is **zero or low** (≤ 4),
   audit each flagged line — if they are genuine missing enumerations,
   fix them in the same commit as the vocabulary expansion. If they
   are imperative or negation phrasings, add the appropriate opt-out
   comment to the line in question.
3. If the count is **high** (≥ 5), the new noun has a false-positive
   problem intrinsic to the project's existing prose — revert the
   vocabulary expansion and instead add an explicit unit test that
   locks the gap in (e.g., `trigger_rejects_bare_<noun>_intentionally`).
   Update the Intentional Gaps note above to document the decision
   and cite the unit test.
4. Never ship a vocabulary expansion that requires a sweeping
   opt-out cleanup across unrelated files. The curated-closed
   philosophy prefers to miss a novel phrasing over introducing
   mass false positives.

The false-positive sweep converts an adversarial finding about a
missing noun from a one-line fix into a deliberate decision with
evidence. It also makes gap documentation cheap: the unit test and
the Intentional Gaps note are the entire artifact.
