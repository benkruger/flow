# Comment Quality

Comments describe the current codebase — what exists, why it exists,
and what it guards. Never write comments that reference a prior
implementation, a deleted codebase, or historical state as the
explanation for current behavior.

## Prohibited Patterns

- **Parity references** — "matches X", "same as X", "mirrors X"
  where X is a deleted file, function, or codebase. The referenced
  thing no longer exists; the comment explains nothing.
- **Historical provenance** — "Removed in PR #NNN", "added in
  commit abc123", "used to be X". Git history is the authoritative
  record; comments that duplicate it go stale.
- **Origin stories** — "Port of test_foo.py", "based on the old
  implementation". Describes where code came from, not what it does.
- **"Before the fix" narratives** — "Before this fix, X would
  happen". Regression test comments should describe what the test
  guards against, not what was broken.
- **"No longer" descriptions** — "X no longer does Y". Describes
  past behavior instead of current contracts.
- **Dead section markers** — "--- X removed in PR #NNN ---".
  Gravestones for deleted code belong in git history, not inline.

## Exception

Tombstone test comments that follow the `Tombstone:.*PR #(\d+)`
pattern are intentional — they reference PR numbers by design for
the tombstone audit system. Do not rewrite these.

## The Forward-Facing Test

Before writing a comment, apply this test: "Does this comment make
sense to someone who has never seen any prior version of this code?"

- If yes — the comment is forward-facing. It describes what exists.
- If no — the comment is backward-facing. Rewrite it to describe
  the current behavior, the invariant being enforced, or the reason
  the code exists as it does today.

## How to Apply

When writing or reviewing comments:

1. State what the code does or why it exists — not where it came from
2. If a design choice needs justification, explain the constraint or
   trade-off — not the historical sequence of events
3. For regression tests, describe what the test guards against — not
   what was broken before
4. For non-obvious values (timeouts, limits, thresholds), explain
   why the value was chosen — not what another system used

## Enforcement

`tests/tombstones.rs::test_no_backward_facing_comments_in_rust_source`
mechanically enforces this rule at CI time. The scanner walks every
`*.rs` file under `src/` and `tests/`, filters out lines matching the
tombstone exception (`Tombstone:.*?PR #`), and asserts no line contains
any phrase from a curated prohibited-pattern list (covering parity
references to a deleted Python codebase, historical PR provenance,
origin stories, "Before the fix" narratives, and dead section markers).
The scanner self-excludes its own file via canonicalized-path
comparison because it must contain the prohibited patterns as search
input.

The pattern list is curated rather than regex-based: it captures every
phrasing the rule explicitly prohibits, plus the phrasings observed in
the repo at the time the rule was first enforced. Novel phrasings
introduced by future commits are not caught automatically — the rule
itself remains the primary instrument, and the scanner is the
merge-conflict trip-wire that locks in the cleanup. When CI fails on a
new prohibited pattern, prefer rewriting the comment to describe
current behavior over expanding the pattern list. When CI fails on a
legitimate forward-facing comment that nonetheless contains a
prohibited substring, narrow the comment's wording or add a more
specific rule exception in the same commit.
