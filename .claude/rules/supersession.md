# Supersession

When a PR introduces code that makes other code elsewhere in the
repository permanently redundant, the redundant code must be deleted
in the same PR. This rule runs in two phases: Plan catches supersession
by construction; Code Review catches it by triage.

## The Test

**If deleting the code leaves the PR's behavior unchanged, the code
is superseded.**

Superseded code is deleted in the PR that supersedes it — not tracked
as follow-up, not left in place, not filed as tech debt. The author of
the PR is the only session that has the context to recognize
supersession cheaply. A future session must re-derive the reasoning
from scratch at the full cost of another lifecycle.

## Shapes to Recognize

- **Authoritative replacement.** A new correct implementation of a
  behavior previously attempted by broken or best-effort code
  elsewhere. The previous attempts become unreachable-in-effect.
- **Deterministic guard.** A new check at an entry point that makes
  downstream defensive handling of the same invalid state impossible
  to trigger.
- **Unified handler.** A new code path that replaces multiple
  specialized code paths. The specialized paths become unreachable.
- **Deprecated API.** A new API that supersedes an old API once the
  switchover lands in the same PR. The old API becomes unreachable.
- **Removed feature with downstream consumers.** A deleted SKILL.md,
  public function, state-file writer, or configuration axis whose
  output (a breadcrumb file, a state field, an emitted event) is
  read by code elsewhere. The consumer becomes orphan infrastructure
  the moment the producer is deleted, even though no test or
  build error surfaces immediately.

These shapes share a pattern: the new code (or the absence of the
deleted code) introduces a contract the existing consumer cannot
strengthen or falsify.

## Plan Phase

When designing a PR that adds a replacement, backstop, guard, or
unified handler, enumerate the code it will supersede during
Exploration. Include deletion tasks in the Tasks section for every
file containing superseded code. List superseded files in the
Exploration table alongside newly-authored files.

A plan that describes a new implementation without listing the code
it makes redundant is incomplete. The Plan phase is where supersession
is cheapest to catch — the Exploration budget is already spent, and
deletion is a mechanical task no different from the implementation
task itself.

### Cascading Deletion Analysis

When the PR's primary action is a deletion (a SKILL.md removal, a
public function removal, a state-field writer removal, a
configuration axis removal), the Plan phase must perform the
inverse supersession analysis: trace every consumer of the
deleted output and classify it. The deletion of a producer
strands every consumer that has no other producer feeding it.

For every deleted artifact in the plan, run a structural sweep:

1. **Identify the artifact's outputs.** What does the deleted
   code produce that other code reads?
   - A SKILL.md may write a breadcrumb file (e.g., a state-file
     field, a `.flow-states/<purpose>.json` marker, a label on
     a GitHub issue).
   - A public function may be the sole writer of a struct field,
     a global, or a side-effect channel.
   - A state-field writer may be the sole producer of a field
     that hooks or downstream subcommands read.
   - A configuration axis may be the sole authorization for a
     code path that depends on the permission.

2. **Grep for consumers of each output.** For each identified
   output, search the codebase for readers — `read_to_string`,
   `get(field_name)`, JSON-key lookups, file-existence checks,
   subprocess invocations of the deleted SKILL or command.

3. **Classify each consumer.** For every reader found:
   - **Has surviving production paths.** Other code still
     produces this output via legitimate paths. The consumer
     keeps working; no action.
   - **Becomes orphan infrastructure.** No remaining producer
     feeds this consumer. The consumer is dead code on merge —
     route it to the Removal section of the plan.
   - **Becomes partially orphan.** Some production paths
     remain, others are removed. The consumer's untaken
     branches lose coverage; flag as a coverage-gap risk for
     the Code phase to address (likely with a replacement test
     for the surviving path).

The classification table belongs in the plan's Exploration or
Risks section so the Code phase has a checklist of orphan
infrastructure to delete in the same atomic commit as the
producer.

A removal plan that fails the cascading deletion analysis ships
orphan code into main. Code Review will catch it (the supersession
test in Step 3 fires regardless of which file the orphan lives
in), but the catch costs a full Code Review cycle plus a deletion
commit that the Plan phase could have included in the original
removal commit.

## Code Review Phase

When triaging findings from agents, apply the supersession test
BEFORE the Real / False positive classification (see
`.claude/rules/code-review-scope.md`).

For every real finding, ask: **"Would deleting the code this finding
describes leave the PR's behavior unchanged?"**

- **If yes** → the finding is in-scope for deletion regardless of
  which file the code lives in. Route to the Fix step.
- **If no** → classify as Real (fix in Step 4) or False positive
  (dismiss with rationale).

The supersession test runs before classification. A file that is not
in the PR diff can still be in-scope if its contents are dead code the
PR created.

## Why Not Track as Follow-Up

Filing a follow-up issue to delete superseded code has three costs:

1. The current session already has the context to recognize and
   delete the code. A future session must rediscover it.
2. The code sits in the repository as tech debt that every subsequent
   reader must classify: still needed, or dead? That classification
   is more expensive than the original deletion.
3. The follow-up issue itself is work: triage, plan, implement,
   review, merge. For a mechanical deletion that the current session
   can do in one edit, the follow-up path is orders of magnitude more
   expensive.

The lowest-cost path is always: recognize supersession, delete in the
current PR, move on.
