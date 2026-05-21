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

## Cross-Surface Application

The obey-vs-describe test above is the upstream classification
gate. Three FLOW surfaces apply it downstream when scanning,
proposing, or filing CLAUDE.md changes: the flow-review
documentation tenant, the flow-hygiene rule audit, and the
flow-doc-sync drift check. Each applies the same test, but the
trigger differs: flow-review fires when a Code-phase change
introduces CLAUDE.md prose, flow-hygiene fires when a
project-local rule file mandates CLAUDE.md prose, and
flow-doc-sync fires when an existing CLAUDE.md section duplicates
content derivable from another source.

**A project-local rule that mandates CLAUDE.md prose for
descriptive content does not override this rule. The mandate is
itself the misclassification.** A rule file that says "X must be
documented in CLAUDE.md" or "the documentation home is CLAUDE.md"
is itself describing how the system works — the obey-vs-describe
test classifies it as descriptive content that belongs in a
feature-specific `.claude/rules/<feature>.md` file plus a one-line
CLAUDE.md index entry, not as authoritative CLAUDE.md prose. The
mandate inverts the routing: it tells the model to add descriptive
content where only behavioral content belongs.

### flow-review documentation tenant

The Review documentation agent (`agents/documentation.md`)
applies the obey-vs-describe gate before emitting any Tenant 6
finding that proposes adding prose to CLAUDE.md. For every
candidate finding the agent considers:

- **Descriptive content** — schema columns, function names,
  helper signatures, code internals, design rationale, file
  paths — routes to a feature-specific
  `.claude/rules/<feature>.md` file plus a one-line CLAUDE.md
  index entry. The finding's Recommendation must name the rule
  file destination and the index-entry shape.
- **Behavioral content** — obey-shape pointers ("X must use Y",
  "all timestamps via Z", "never invoke W directly") — routes
  to CLAUDE.md directly as a behavioral instruction or pointer
  line.

A finding that proposes adding descriptive prose to CLAUDE.md is
itself a misclassification. The fix is the routing change, not
the prose addition.

### flow-hygiene mandate scan

The flow-hygiene skill (`skills/flow-hygiene/SKILL.md`) scans
project-local `.claude/rules/*.md` files for paraphrased patterns
that mandate CLAUDE.md prose for descriptive content — phrasings
like "treats X added without Y documented in CLAUDE.md", "must be
documented in CLAUDE.md", "documentation home is CLAUDE.md",
"CLAUDE.md as the canonical destination". Matches emit
`[CLAUDE_MD_MANDATE]` findings. The fix routes the mandated
prose to a feature-specific `.claude/rules/<feature>.md` file
with a one-line CLAUDE.md index entry per this rule. Matches
inside quoted-example fences or paragraphs explicitly naming the
pattern as an anti-pattern are excluded by construction.

flow-hygiene additionally enforces a configurable CLAUDE.md size
budget (`.flow.json` field `claude_md_budget`, defaults 12000
chars / 400 lines). When CLAUDE.md exceeds either budget, the
skill emits a `[SIZE_BUDGET]` finding pointing at this rule for
the routing pattern — descriptive prose should land in
feature-specific rule files, not in CLAUDE.md itself.

### flow-doc-sync duplication detection

The flow-doc-sync skill (`skills/flow-doc-sync/SKILL.md`) scans
CLAUDE.md sections for paragraphs that duplicate content
derivable from schema files, source docstrings, or existing rule
files. When a 3+ sentence description-shape paragraph names 3+
identifiers (table names, function names, helper signatures,
file paths) all reachable elsewhere, the skill emits a
`[DUPLICATE]` finding. The recommended fix routes the prose to a
feature-specific `.claude/rules/<feature>.md` file and reduces
the CLAUDE.md section to a one-line index entry per this rule —
the description-shape content does not satisfy the
obey-vs-describe test, so CLAUDE.md is not its destination.
Behavioral-imperative paragraphs are excluded by construction.
