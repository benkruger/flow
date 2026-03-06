# Skill Authoring

## Phase Structure

When adding a phase, audit back-navigation in all adjacent skills.
Inserting a new phase shifts numbering. Every "Go back to Code" or
"Go back to Plan" instruction in adjacent skills must reset all
intermediate phases, including the new one.

## Permission Safety

Check the deny list before writing git commands in skills. `git
checkout` is forbidden even for file-level operations. Use `git
restore` instead. Before adding any git command to a skill's bash
blocks, verify it does not match a deny-list pattern in
`.claude/settings.json`.

Test permission changes before committing. If you cannot verify
whether a pattern is valid or will be honored, say so and propose
a testable alternative.

## Commit Skill Internals

Never skip `git add -A` in flow:commit Step 1. The Code phase
task review shows diffs via `git diff HEAD`, which displays
unstaged changes without staging them. The commit skill must
always run `git add -A` before `git diff --cached`.

Never run `git add -A` in commit Step 4. Files are already
staged from Step 1. Running it again stages `.flow-commit-msg`
itself, causing it to be tracked in the commit.

## Sub-Agent Safety

Never use `bypassPermissions` mode on sub-agents. Permission deny
lists exist to prevent destructive operations. Always use the
default mode. If a sub-agent needs a denied permission, surface it
to the user.

## Safety Checks

Never suggest removing safety checks. If performance is a concern,
propose making it faster, not removing it.

## Unexpected Test Failures

When bin/ci reveals an unexpected conflicting test, report before
fixing. Name the conflicting test, explain why it conflicts, and
describe the fix. Do not silently expand the scope.

## Codebase-Wide Renames

When planning a rename of phase names, skill names, or commands:
always audit CLAUDE.md explicitly — it is hand-maintained and
frequently contains command references, phase name prose, and
convention entries that don't surface in automated grep-based scope
analysis. Missed CLAUDE.md references cause user-visible doc drift.
