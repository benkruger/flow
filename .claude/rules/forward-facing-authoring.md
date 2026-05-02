# Forward-Facing Authoring

Every persistent artifact a FLOW phase produces — rule files,
CLAUDE.md entries, doc comments, skill instructions, plan prose,
finding reasons — describes what IS, not what WAS. The incident
that motivated the artifact is fixed; future readers do not need
to reconstruct the incident to understand the guidance.

## Scope

Applies to:

- `.claude/rules/*.md`
- `CLAUDE.md`
- Doc comments on Rust items in `src/**/*.rs`
- Skill instructions in `skills/**/SKILL.md` and
  `.claude/skills/**/SKILL.md`
- Plan prose (Context, Exploration, Risks, Approach, Tasks)
- Finding records where the `reason` will be re-read as guidance
  (not just as triage audit)

Exempt (historical by nature):

- Commit messages
- GitHub issue bodies
- Session logs (`.flow-states/<branch>/log`)
- State-file `findings[]` `reason` fields that record per-PR
  triage rationale and are not re-read as future guidance
- Tombstone test comments (follow the `Tombstone: ... PR #<N>`
  pattern by design — see `.claude/rules/tombstone-tests.md`)

## The Test

Before writing, apply the Forward-Facing Test:

> "Does this make sense to a future reader who has never seen
> the incident that produced it?"

- Yes — keep.
- No — rewrite to describe the principle, behavior, or
  constraint. Remove the history.

## Prohibited Patterns

- First-person narrative: "we hit this", "I found", "during
  PR #NNN", "in branch X"
- "After the fix" framing: "now that X is fixed, don't…"
- "Was/was-not" framing: "previously the code did X",
  "until recently we…"
- Just-merged-work citations: a specific file or PR the current
  session just touched, used as the anchor for the principle
- Principles derived from a single incident with no
  generalization beyond it
- Motivating-incident sections appended to rules — provenance
  belongs in commit messages, not rule bodies

## Correct Patterns

- Name the code shape, behavior, or process abstractly, then
  illustrate with a generic snippet
- State the invariant, contract, or required behavior
- Use present tense: "When X happens, do Y"
- Cite code references only when they are permanent canonical
  examples — a reference implementation that will outlast any
  individual incident

## How Learn Applies This

Learn's audit findings are backward-facing by construction
("during this flow, X happened"). The rule Learn writes must
not be.

Transform findings into rules by stripping the incident:

- Finding (backward): "During PR #NNN, function X had an
  unreachable branch that led the session to ship <100%."
- Rule (forward): "When an early-return guard guarantees a
  downstream invariant, later code must not re-check the
  invariant via a pattern that produces an unreachable branch.
  Use value substitution or fold into existing control flow."

If the rule only makes sense when the reader knows the specific
incident that spawned it, the finding is too
incident-specific to codify. Record it in the commit message
and discard — do not create a rule file.

## How Code Review Applies This

Fixes in Step 4 that add doc comments or rule updates must be
forward-facing before commit. A rule addition or doc comment
that names the current PR, branch, or file the session just
modified is a Real finding that must be rewritten before Step 4
concludes.

## Cross-References

- `.claude/rules/comment-quality.md` — the same Forward-Facing
  Test applied to inline code comments specifically
- `.claude/rules/tests-guard-real-regressions.md` — tests must
  guard a named regression; this rule's equivalent for prose
  artifacts is to name a general principle, not a specific PR
- `.claude/rules/no-waivers.md` — prohibits waiver
  documentation; this rule prohibits backward-facing rule
  documentation more broadly
