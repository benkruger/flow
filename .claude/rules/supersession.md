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

These shapes share a pattern: the new code introduces a contract the
existing code cannot strengthen or falsify.

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

## Code Review Phase

When triaging findings from agents, apply the supersession test
BEFORE the diff-boundary test (see `.claude/rules/code-review-scope.md`).

For every real finding, ask: **"Would deleting the code this finding
describes leave the PR's behavior unchanged?"**

- **If yes** → the finding is in-scope for deletion regardless of
  which file the code lives in. Route to the Fix step. Do not file
  an issue.
- **If no** → apply the diff-boundary test as usual.

The supersession test overrides the diff-boundary test. A file that
is not in the PR diff can still be in-scope if its contents are dead
code the PR created.

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
