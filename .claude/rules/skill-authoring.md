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

Never use `general-purpose` sub-agents in skills — they ignore
tool restriction rules in their prompts. Use custom plugin
sub-agents with `PreToolUse` hooks for system-level enforcement.
See `agents/ci-fixer.md` for the pattern: the hook
(`lib/validate-ci-bash.py`) blocks compound commands and
file-read commands with exit code 2, feeding helpful error
messages back to the sub-agent so it adapts.

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

## Plan Task Ordering

Every plan must include test tasks — even for pure-markdown skills,
add contract tests in `test_skill_contracts.py`. TDD means the test
task comes before the implementation task it validates.

## Negative-Assertion Test Compatibility

When writing a SKILL.md instruction that prohibits a specific string
(e.g. "do not use --comment"), phrase the prohibition without including
the literal prohibited string. Contract tests like
`test_code_review_does_not_use_comment_flag` scan the entire SKILL.md
content — the prohibition text itself will trigger the assertion failure.
Use paraphrased instructions such as "invoke with no flags or arguments"
instead of "do not pass the --comment flag."

## Codebase-Wide Renames

When planning a rename of phase names, skill names, or commands:
always audit CLAUDE.md explicitly — it is hand-maintained and
frequently contains command references, phase name prose, and
convention entries that don't surface in automated grep-based scope
analysis. Missed CLAUDE.md references cause user-visible doc drift.

## Cleanup Script Step Ordering

When adding a new step to `lib/cleanup.py` that operates on files
inside the worktree, place it BEFORE the worktree removal step.
The `git worktree remove` call deletes the entire directory tree —
any step that reads or removes worktree-internal files must precede
it or the target path will not exist.

## Fenced Code Blocks Before Closing Tags

When a bash block ends immediately before a closing XML-like tag
(`</SOFT-GATE>`, `</HARD-GATE>`), add a blank line between the
closing ` ``` ` and the tag. pymarkdown MD031 requires a blank line
after every fenced code block, including when the next line is a
closing tag rather than prose.

## Destination Renumbering

When renumbering destinations or steps within a SKILL.md, grep for the
old numbers throughout the entire file before marking the change complete.
Preamble summary lines (e.g. "Use `<worktree_path>` for destinations 2
and 4") are easy to miss because they sit far from the destination table
they reference. A grep for the old number catches these stale references.

Also audit skip/jump targets — instructions like "Skip directly to
Step 8 (cleanup)" that reference steps by number. When inserting a new
step, these targets must be reconsidered for intent, not just
mechanically incremented. A skip that pointed to cleanup before the
insertion should now point to the new step if the new step should also
run in that path.

## Mid-Phase Self-Invocation

When a phase skill invokes built-in skills (Skill tool) mid-phase and
must continue after the built-in skill returns, use self-invocation —
not HARD-GATEs. HARD-GATEs are instructional Markdown that the model
ignores at Skill tool turn boundaries. The correct pattern: after each
sub-step completes, invoke the skill itself as the FINAL action with
a `--continue-step` flag. The skill's Resume Check reads a step counter
from the state file and dispatches to the next sub-step on re-entry.
This mirrors how phase-to-phase transitions work — the Skill invocation
is the last action, never a mid-response call.
