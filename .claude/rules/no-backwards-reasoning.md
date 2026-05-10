# No Backwards Reasoning

Decisions about current code stand on their current merits and
what the code should do, not on the history of how the code
arrived at its current state. Reasoning that grounds a present
choice in a past commit message, PR description, or doc comment
that justified the prior shape is forbidden — history records
what was decided, not what is correct now.

## The Trip-Wire

Before reading any historical artifact — a commit message, a PR
description, a doc comment that justifies past behavior, the
output of `git log`, the output of `git blame`, an old issue
thread — name the question being answered. If the question is
"what should this code do?" or "what should this PR include?",
STOP. History cannot answer should-questions. It can only tell
you what someone previously decided; it cannot tell you whether
that decision is still correct.

The trip-wire applies in both directions:

- **Reading.** Do not consume historical artifacts as authority
  when answering a should-question.
- **Citing.** Do not cite a historical artifact in prose
  (issue body, plan section, finding reason, comment) as the
  justification for a present choice.

## Forbidden Reasoning Patterns

Each pattern below is a backward-facing reasoning shape. The
forbidden form is the one that grounds the current decision in
the historical artifact; the reasoning is incomplete because it
substitutes "this is what was decided" for "this is what is
correct."

| Pattern | Example phrasing |
|---|---|
| Historical decision cited as authority | "PR #NNN decided X, so we must X" |
| Deferring to a prior author | "the prior PR chose Y, so this PR follows" |
| Doc-comment provenance as constraint | "the doc comment says it was added for Z, so keep Z" |
| `git blame` as design rationale | "blame shows this line came from PR #NNN, so do not change it" |
| `git log` as a should-source | "the log shows we did this last quarter, so do it now" |
| "Kept for backward compatibility" without a current consumer | "preserve this branch in case something old reads it" |
| "Compat shim" / "legacy alias" without a named consumer | "leave the alias for older code paths" |

In every row, the fix is to ask the should-question directly:
what does the current codebase need this code to do? If the
answer requires the historical shape, the rationale is the
current need — not the historical decision.

## Plugin-Version-Compat Sub-Case

The FLOW plugin auto-updates from the marketplace. There is no
installed base of older plugin versions running against newer
state files; every session of every user runs the latest
plugin version. As a consequence, the following defensive
patterns are forbidden because they exist only to support
hypothetical older plugin versions that cannot exist in
practice:

- `serde` field aliases that accept both an old and a new key
  for the same field
- Fallback readers that try `state["new_field"]` and fall back
  to `state["old_field"]`
- Dual-key parses where `Option<String>` is filled from
  whichever key is present
- Tests that lock in compat behavior — "old plugin writes X,
  new plugin reads X-or-Y"

When a plan task or Code-phase change proposes any of the
above, the trip-wire fires: the should-question is "what does
the current code need to read?" and the answer is "the current
key only." The compat layer is reasoning grounded in plugin
history that no production caller can produce.

The same applies to state-file shapes: there is no migration
window, no legacy reader, no need to accept obsolete keys.
Writers produce the current shape; readers consume the current
shape.

### Key-fallback vs. type-tolerance

The forbidden patterns above target **key-fallback** — code
that tries one field name and, on absence, falls back to a
different field name from a prior schema. That is reasoning
grounded in plugin history.

**Type-tolerance for the same field is not forbidden** and is
required by `.claude/rules/state-files.md`. A reader that
accepts integer, float, and string representations of the same
counter field (via `tolerant_i64` / `tolerant_i64_opt` per
`.claude/rules/rust-patterns.md` "Counter and State Field Type
Tolerance") is normalizing input shapes for one field, not
falling back across renamed fields. Writers always produce
integers; readers tolerate the alternative representations the
JSON serializer or hand-edits can introduce. This is current-
merits reasoning — the current code reads a counter and the
current field can hold any of those representations.

The distinguishing test: does the alternative source the reader
falls back to come from a different field name? If yes →
key-fallback → forbidden. If the alternative is just a
different in-memory representation of the same field → type-
tolerance → required.

## Valid Uses of History

History is not banned — it is forbidden as authority. These
uses remain valid:

- **Forensic detection.** "Did PR #NNN merge?" or "when did
  this regression first appear?" are factual questions about
  events that happened. Tools like `git log`, `gh pr view`,
  and `git blame` answer them directly.
- **Understanding intent as a question to re-evaluate.**
  Reading a prior PR description to learn what was intended is
  fine — as long as the next step is "is this intent still
  correct?" rather than "this intent must be preserved."
- **Audit trails.** Commit messages, session logs, and tombstone
  comments are appropriate places to record history. Per
  `.claude/rules/forward-facing-authoring.md`, those surfaces
  are exempt from forward-facing prose discipline because they
  exist to record what happened.

The distinguishing test: is the historical artifact answering a
factual question (what happened) or a normative question (what
should happen)? Factual is fine. Normative is forbidden.

## Surfaces Where the Rule Fires

The rule applies at every surface where reasoning produces
durable output:

- **Reasoning** (primary). When designing a fix, drafting a
  plan, classifying a finding, or composing an issue body, do
  not consume historical artifacts as authority on
  should-questions.
- **Issue drafting** (mechanical backstop). The
  `flow-create-issue` skill scans candidate issue bodies for
  forbidden phrasings before presenting the draft, and the
  `flow-decompose-project` skill scans each child issue body
  before children are surfaced. The scan is a backstop for
  cases where backward-facing reasoning slips into the prose
  despite the primary discipline.
- **Code phase is too late.** Once the implementation is
  written from a backward-facing premise, Code Review can
  catch the symptom but not the reasoning. The rule fires in
  the Plan and earlier phases, not after the diff exists.

### Canonical Scan Phrasings

The mechanical backstop scans for these four canonical
phrasings. Future changes to the scanned set must update both
this enumeration and the corresponding subsections in
`skills/flow-create-issue/SKILL.md` and
`skills/flow-decompose-project/SKILL.md` so the rule remains
the authoritative source for what the scans target:

- `"PR #<N> decided"` — historical decision cited as authority
- `"kept for backward compatibility"` — preservation justified
  by inherited reasoning rather than a current consumer
- `"older plugin versions"` — plugin-version-compat reasoning
- `"as PR #<N> chose to"` — deferring to past decisions

## Cross-References

- `.claude/rules/forward-facing-authoring.md` — sibling rule
  covering AUTHORING. Forward-facing authoring forbids writing
  history-citing prose; this rule forbids reading history as
  authority. Together they close the loop.
- `.claude/rules/comment-quality.md` — sibling rule for
  comment writing. Backward-facing comments are forbidden by
  the same principle: the comment must describe what the code
  does now, not what it used to do.
- `.claude/rules/investigate-root-cause.md` — sibling rule for
  investigation discipline. When a bug surfaces, investigate
  the system as it stands; do not rationalize the current
  behavior from a historical decision.
- `.claude/rules/state-files.md` — sibling rule covering type-
  tolerance for state-file fields. The "Key-fallback vs.
  type-tolerance" subsection above distinguishes the forbidden
  shape (key-fallback across renamed fields) from the required
  shape (type-tolerance for one field).
