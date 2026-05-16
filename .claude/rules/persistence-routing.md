# Persistence Routing

**Rules are the default. Memory is the exception. CLAUDE.md is
the smallest of the three.**

When the user says "do X", "never do Y", or "when X happens do Y" —
that is a behavioral constraint. Behavioral constraints are rules,
not memory. Memory exists for a narrow case: information specific
to *this user* that no other engineer working on the project would
need. CLAUDE.md is reserved for behavioral instructions the model
obeys plus pointer indexes to rule files — every other shape of
project knowledge has a dedicated destination.

## Decision Tree

In order:

1. **Is it a behavioral constraint?** (do X, never do Y, when X
   happens do Y — an imperative guardrail) → **Rule**
   (`.claude/rules/<topic>.md` via `bin/flow write-rule`)
2. **Is it project knowledge?** Apply the **obey-vs-describe test**:
   - **Obey** — the model must follow this directive every session
     (e.g. "All timestamps use Pacific Time via `now()`") →
     **CLAUDE.md** as a behavioral pointer line.
   - **Describe** — this explains how something works (architecture
     mechanics, code internals, design rationale) → route to a
     module doc comment, the `docs/` subtree, or discard:
     - **Module doc comment** in `src/<name>.rs` — Rust code
       mechanics that future readers find via grep or rustdoc.
     - **`docs/` subtree** — long-form architecture, schema
       reference, public-facing material.
     - **Discard** — when the Discoverability test resolves
       negatively, the next session can derive the content by
       reading the code or existing rules.
3. **Is it specific to this user, not the project?** (the user's
   role, communication style preferences, personal corrections
   that no other engineer would need) → **Memory**

The order matters. When a piece of guidance fits more than one
category, it goes to the earliest matching destination — Rule wins
over CLAUDE.md, CLAUDE.md wins over Memory.

## Tests

Apply each test in order. The first one that resolves wins.

- **Imperative test.** Can you phrase it as "do X" / "never do Y"
  / "when X, do Y"? → Rule. The user's phrasing does not have to
  be imperative for the underlying guidance to be one.
- **Obey-vs-describe test.** Does the model OBEY this every
  session (behavioral pointer), or does this DESCRIBE how
  something works (architecture mechanics, design rationale)? →
  Obey routes to CLAUDE.md; describe routes to a
  module doc comment, the `docs/` subtree, or discard.
- **Forward-applicability test.** If a future engineer working on
  this project encountered the same situation, would they need
  this guidance? → Rule. The audience is the project, not the
  current user.
- **User-specific test.** Is this guidance about *this user
  specifically* — their role, their preferred working style,
  their personal context — that another engineer would not need?
  → Memory.
- **Discoverability test.** Can the next session derive this by
  reading code, CLAUDE.md, or existing rules? → Don't save it.

## What CLAUDE.md Is For

CLAUDE.md carries exactly two content shapes:

- **Behavioral instructions the model obeys.** Imperatives that
  bind every session in this project. Example: "All timestamps
  use Pacific Time via `src/utils.rs::now()`."
- **Pointer indexes to rule files.** One-line cross-references
  that name a topic and direct readers to the rule file that
  owns the detail. Example: "**Tombstone tests** — see
  `.claude/rules/tombstone-tests.md`."

CLAUDE.md is always loaded into every session's context. Every
byte costs token budget on every subsequent turn for every
engineer working in the project. The two shapes above earn their
place by binding behavior or by serving as the discovery surface
for deeper detail.

## What CLAUDE.md Is Not For

CLAUDE.md must never carry descriptions of how the system works.
Architecture mechanics, design rationale, code internals — these
are descriptive, not behavioral, and belong in one of three
alternative destinations:

- **Module doc comment** in `src/<name>.rs` — describes Rust code
  mechanics where future readers arrive via grep or rustdoc. The
  comment lives with the code it describes so a refactor cannot
  silently make it stale.
- **`docs/` subtree** — long-form architecture, schema reference,
  public-facing material. Loaded on demand by readers who need
  the detail, not by every session.
- **Discard** — when the Discoverability test resolves negatively,
  the next session can derive the content by reading the code or
  existing rules. Recording the derivation in CLAUDE.md compounds
  token cost on every session for content the next session would
  reconstruct anyway.

The obey-vs-describe test is the gate. A candidate addition that
fails the test routes to one of the three destinations above —
not to CLAUDE.md.

## Common Misclassification

The most common error is treating "the user said never to do X"
as automatically a memory entry. The user's phrasing is not the
classification signal — the audience is. A user's correction in
one session is a shared discovery that usually applies to every
engineer working on the project; it is not user-private just
because the user happened to be the one who surfaced it. "Never
use raw `git commit`; always invoke `/flow:flow-commit`" sounds
personal in a correction, but every engineer working on FLOW
needs that constraint. It is a rule.

The forward-applicability test catches this: if a future engineer
working on this project would also need the guidance, it is a
rule, not a memory. A behavioral constraint that affects how the
codebase evolves belongs in `.claude/rules/`, where every session
on every branch sees it. Memory is invisible to other engineers
and to the model in target projects.

When in doubt, write the rule. A rule that turns out to be
user-specific can be reclassified later — delete the rule from
the repo and ask the user to add the equivalent text to
`~/.claude/CLAUDE.md` themselves. There is no automated migration
path; the conversion is manual but always available. A memory
entry that should have been a rule is invisible until the next
session re-derives it from scratch, so the asymmetry favors
defaulting to rules.

## Never Store in Memory

- Behavioral constraints — those are rules
- Architecture, code facts, or file paths — read the code
- Duplicates of existing rules or CLAUDE.md content
- Git history or debugging solutions
- Ephemeral task state

## How to Persist a Rule

Edits to `.claude/rules/<topic>.md` route through `bin/flow
write-rule` during an active flow per
`.claude/rules/file-tool-preflights.md`. Write the rule content
to a temp file under `.flow-states/<branch>/` and invoke
write-rule to land it at the canonical path.

For an entirely new rule topic, name the file after the
constraint's subject (`<topic>.md` — e.g.,
`always-verify.md`, `no-waivers.md`) and follow the
forward-facing prose discipline in
`.claude/rules/forward-facing-authoring.md`.
