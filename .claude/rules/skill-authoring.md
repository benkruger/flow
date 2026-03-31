# Skill Authoring

## Simplest Approach First

When designing a skill change, start with the simplest solution that
works. If the user proposes a simple approach, do not add machinery
(resume checks, self-invocation, state counters) unless you can
explain in one sentence why the simple approach fails. If you cannot
articulate the failure, the simple approach is correct.

When you agree to simplify and then re-introduce the same complexity
in the next response, you are flip-flopping. Stop, re-read what you
agreed to, and follow through.

## Phase Structure

When adding a phase, audit back-navigation in all adjacent skills.
Inserting a new phase shifts numbering. Every "Go back to Code" or
"Go back to Plan" instruction in adjacent skills must reset all
intermediate phases, including the new one.

## Flat Sequential Step Numbering

All steps in a SKILL.md must use flat sequential `### Step N` headings.
Never use sub-step labels (1a, 1b, 2a–2g) or bold sub-step markers
(`**2a.`). When a group of steps shares a logical context (e.g. steps
that run under a lock), use a prose preamble before the first step in
the group instead of nesting them under a parent step.

## Permission Safety

Check the deny list before writing git commands in skills. `git
checkout` is forbidden even for file-level operations. Use `git
restore` instead. Before adding any git command to a skill's bash
blocks, verify it does not match a deny-list pattern in
`.claude/settings.json`.

Test permission changes before committing. If you cannot verify
whether a pattern is valid or will be honored, say so and propose
a testable alternative.

## Platform Constraints

Claude Code has built-in protections that cannot be overridden by
settings.json entries. `.claude/` paths are protected regardless
of `defaultMode` or allow-list patterns. When a permission prompt
persists despite allow-list entries, the cause is a platform
constraint — not a missing permission. Look for existing bypasses
(like `write-rule.py` for `.claude/` writes) before proposing
new solutions. Never propose adding permissions for paths that
are platform-protected.

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
sub-agents with the global `PreToolUse` hook for system-level
enforcement. The hook (`lib/validate-pretool.py`) is registered
in `hooks/hooks.json` and blocks compound commands and
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

## Decompose Completeness

When the user makes a material correction to the approach after the
initial decompose run, re-run decompose with the complete corrected
understanding before writing the plan. A decompose based on partial
understanding produces a plan that looks correct but was never
validated against the full design. Do not patch the plan manually —
the decompose must see the complete algorithm.

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

Similarly, any SKILL.md command that reads `.flow-states/` files
(state file, log, CI sentinel) must be placed in a numbered step
BEFORE the cleanup step. The Done section runs after cleanup — by
that point, `.flow-states/<branch>.json` has been deleted and any
command that reads it will fail.

## Numbered Lists With Fenced Code Blocks

Never use numbered lists (1. 2. 3.) when fenced code blocks appear
between items. pymarkdown MD029 treats each code block as a list
interruption, resetting the expected prefix. Use bold paragraph
headers (**Step name.**) instead of numbered items when steps
contain code blocks.

## Fenced Code Blocks Before Closing Tags

When a bash block ends immediately before a closing XML-like tag
(`</SOFT-GATE>`, `</HARD-GATE>`), add a blank line between the
closing ` ``` ` and the tag. pymarkdown MD031 requires a blank line
after every fenced code block, including when the next line is a
closing tag rather than prose.

## Decision Point Gates

Every user decision point in every skill — phase or utility — must be
wrapped in `<HARD-GATE>` tags with explicit enforcement language. Prose
instructions like "ask the user" or "use AskUserQuestion" are
insufficient on their own. Without the HARD-GATE wrapper, Claude treats
approval prompts as suggestions that can be bypassed when the answer
seems obvious — especially after extended discussion where a solution
has already been explored.

The HARD-GATE must prohibit all action without explicit user approval:
proceeding to the next step, proposing direct edits, committing changes,
or taking any action outside the active skill flow. The enforcement
language is what distinguishes a gate from a suggestion.

## Safe Defaults for Subjective Classification

When a skill asks the model to classify conversation content (e.g.,
"is this output implementation-focused?"), include an explicit
tiebreaker for ambiguous cases. The safe default is always the
conservative action — the one that produces correct behavior even
if the classification is wrong.

## Contract Test Atomicity in Plan Dependencies

When a plan removes content that a contract test asserts exists, and a
later task re-adds it at a different location, the plan must mark those
tasks as atomically dependent — they must be in the same commit. Otherwise
CI fails in the intermediate state when the content is absent.

Before finalizing the dependency graph, check every removal task against
`test_skill_contracts.py` assertions. If any assertion validates the
presence of the removed content, pair the removal with the re-addition
task.

## Destination Renumbering

When renumbering destinations or steps within a SKILL.md, grep for the
old numbers throughout the entire file before marking the change complete.
Preamble summary lines (e.g. "Use `<worktree_path>` for destinations 2
and 4") are easy to miss because they sit far from the destination table
they reference. A grep for the old number catches these stale references.

Also audit spelled-out step counts in prose sections (e.g. "six review
steps" in a Framework Conventions paragraph). These do not follow the
`Step N` pattern and are invisible to number-based grep. Search for the
old count as a word ("six", "three", etc.) in addition to as a digit.

Also audit skip/jump targets — instructions like "Skip directly to
Step 8 (cleanup)" that reference steps by number. When inserting a new
step, these targets must be reconsidered for intent, not just
mechanically incremented. A skip that pointed to cleanup before the
insertion should now point to the new step if the new step should also
run in that path.

When a step is moved (not added), range boundaries need special
attention. "Steps 2–11" does not become "Steps 2–12" just because every
reference was mechanically incremented — the total step count is
unchanged if a step moved from one position to another. After all edits,
verify the range endpoint by counting `### Step N` headings in the file.

## Value Replacements in Prose

When replacing a value in code (e.g. swapping one entry in a list for
another), grep the entire SKILL.md for the old value — not just the
lines the plan identifies. Prose descriptions of what the code does
(e.g. Step 4 describing what a setup script writes) echo the code's
values and are easy to miss when the plan only lists code locations.

## Verify Script Behavior Claims in Issues

When an issue body asserts specific script behavior (e.g. "field X is
populated after Step Y"), verify the assertion by reading the script
source during the Plan phase. Issue authors — including Claude in prior
sessions — can be wrong about what a script does internally. A single
grep of the script for the relevant field or function catches false
assumptions before they become bugs in the implementation.

## Config Chain Integrity

The autonomy config chain is: prime presets → `.flow.json` → state file → skill reads.
Phase skills must read mode resolution from the state file only — never `.flow.json`.
When a phase skill's config is missing at runtime, the fix is always at the source
(add the skill to the prime presets in `flow-prime/SKILL.md`), never at the consumer
(adding `.flow.json` fallback reads to the skill). Every skill in `CONFIGURABLE_SKILLS`
(`test_skill_contracts.py`) must have an entry in all 4 prime presets — CI enforces this.

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

## Target Project Mindset

Every bash block, subprocess call, and file path in a plugin skill
or lib script runs in a target project, not this repo. Before
adding any command, ask: "Does this work in a Rails project with
no `bin/flow`, no `.venv/`, and non-bash `bin/` scripts?" The FLOW
repo is Python with bash scripts — it is the worst possible test
environment for a multi-framework plugin. Integration tests for
lib scripts must use the `target_project` fixture, not `git_repo`.

## Plugin User Reachability

Every new feature — not just skill bash blocks — must have a clear
answer to: "How does a plugin user in a target project access this?"
before implementation begins. If the answer is unclear, the feature
will ship unreachable. Issue #362 is the cautionary example: 27+
commits built a TUI that no plugin user could launch.

There are exactly three valid access paths for plugin users:

1. **Skill** — a slash command (`/flow:flow-xxx`) the user invokes
2. **Hook** — auto-triggered by Claude Code events (SessionStart,
   PreToolUse, etc.)
3. **Global launcher** — a `flow <subcommand>` routed through
   `bin/flow`

If a feature does not fit one of these three paths, it is unreachable
from a target project and must not proceed past planning without a
design that makes it reachable.

## Plugin Root for bin/flow

Every `bin/flow` call in a plugin skill bash block must use
`${CLAUDE_PLUGIN_ROOT}/bin/flow`. Bare `bin/flow` only
resolves in the FLOW repo itself — target projects do not have
it. This works during plugin development (the FLOW repo has
`bin/flow` locally) but fails with exit 127 in every target
project. CI enforces this via
`test_plugin_skills_use_plugin_root_for_bin_flow`.

## Worktree bin/flow for Repo-Modifying Commands

When running repo-modifying bin/flow subcommands (e.g. bump-version) during
the Code phase in a worktree, use the worktree's own bin/flow — not the
cached plugin's ${CLAUDE_PLUGIN_ROOT}/bin/flow. These scripts resolve file
paths relative to __file__, so the cached plugin writes to the cache
directory. FLOW state commands (phase-transition, set-timestamp, log, ci) use
project_root() and work from either path.
