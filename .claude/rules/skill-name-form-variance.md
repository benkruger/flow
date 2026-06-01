# Skill-Name Form Variance in Discriminators

When a hook predicate or transcript walker compares a
transcript-recorded `input.skill` value against a literal to make a
gate decision, it must accept every invocation form Claude Code can
record for that skill — never a single hardcoded form when the skill
can be recorded under more than one.

## Why

The Skill tool records `input.skill` verbatim, and
`extract_skill_invocations` surfaces that raw value unchanged. A
plugin skill resolves under two valid forms: the bare `<name>` and
the namespaced `<plugin>:<name>`. When the model can invoke a skill
under either form, both appear in real transcripts. A discriminator
hardcoding one form (`most_recent.as_deref() == Some("<one-form>")`)
silently fails on the other — and because these predicates fail
open, the guard never fires when it should, with no error surfaced.

The canonical instance: the decompose skill is invoked under both
bare `decompose` and namespaced `decompose:decompose`. A
discriminator that matched only `decompose:decompose` let a
bare-form invocation pass the guard.

## The Rule

A predicate that compares a transcript-recorded `input.skill`
against a literal for a skill the model can invoke under multiple
forms MUST route the comparison through a named helper
`is_<skill>_skill(s) -> bool` that:

1. Normalizes the input via `normalize_gate_input` (NUL strip +
   trim + ASCII-lowercase) per
   `.claude/rules/security-gates.md` "Normalize Before Comparing".
2. Returns true for every valid form (e.g. `n == "decompose" || n
   == "decompose:decompose"`), using full-string `==` — never
   `contains`/`starts_with`, so a superstring (`decompose-foo`,
   `xdecompose`) cannot match.

Reference implementation: `is_decompose_skill` in
`src/hooks/stop_continue.rs`.

## Distinction From Intentional Per-Skill Form Matching

Some discriminators deliberately match ONE specific form because
that skill is only ever recorded under that form. The user-only
`flow-release` skill is recorded bare (`flow-release`);
`flow:flow-commit` is recorded namespaced. Those single-form
comparisons are correct and documented — see
`.claude/rules/user-only-skills.md` and
`.claude/rules/concurrency-model.md`, where the canonical
per-skill form is established for each.

This rule applies ONLY to skills the model can invoke under
MULTIPLE forms. The distinguishing question: can the model produce
this skill name in more than one shape in `input.skill`? If yes →
accept every form via the helper. If no → a single-literal match
is correct.

## Enforcement

The Review reviewer agent flags any new single-literal
`input.skill` comparison for a skill that can be recorded under
multiple forms as a Real finding. There is no mechanical scanner —
the discrimination between "intentional single-form" and
"buggy single-form" requires knowing whether the skill emits
multiple forms, which is author judgment.
