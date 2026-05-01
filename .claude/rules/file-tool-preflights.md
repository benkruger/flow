# File-Tool Preflights

Claude Code's Write tool and Edit tool each have a Read-first-in-session
preflight: Write errors when the target file already exists on disk and
has not been Read in the conversation, and Edit errors when an edit is
attempted before a prior Read on the target. FLOW skills that write to
persistent branch-scoped or project-root paths must route those writes
through the `bin/flow write-rule` Rust subcommand, which does `fs::write`
unconditionally so the preflight cannot fire. Skills that instruct Edits
against named plan or DAG files must precede the Edit with an explicit
Read-tool instruction so the Edit preflight is satisfied even when the
model has not naturally read the file in the current turn.

## The Bug Class

When a skill instructs the model to Write a file that may pre-exist, the
Write tool's preflight fires with "Error writing file" — visible in the
conversation as a red error line under the Write tool call. Pre-existence
comes from:

1. A prior attempt in the same session wrote the file before a retry.
2. The `--continue-step` self-invocation re-enters the skill; prior
   Write's Read-tracking may not survive the reinvocation context.
3. Context compaction during a long turn drops Read-tracking.
4. An aborted earlier session left the file.

Recovery from a preflight block requires manual workaround outside the
normal skill workflow — the model has to invoke Read on the blocked path
before the Write can proceed, which wastes turns and can corrupt the
workflow if the model instead accepts whatever content is already on
disk.

## Monitored Target Paths

Monitored paths are files FLOW skills write to repeatedly across
invocations (branch-scoped or machine-level singletons) or files that
may pre-exist from a prior session on fresh re-entry. Session-scoped
files with a unique `-<id>` suffix are excluded because the id makes
cross-invocation collision unlikely. Writes to the monitored set must
route through `bin/flow write-rule`:

- `.flow-states/<branch>-dag.md` — the decompose-produced DAG file
- `.flow-states/<branch>-plan.md` — the Plan-phase implementation plan
- `.flow-states/<branch>-commit-msg.txt` — the commit skill's message
  file consumed by `bin/flow finalize-commit`. Branch-scoped so
  concurrent flows in different worktrees of the same repo never
  collide.
- `.flow-issue-body` — the shared issue body file (project root)
- `orchestrate-queue.json` — the machine-level orchestration queue
  (in `.flow-states/`)

Session-scoped `-<id>` temp files used by `flow-create-issue` and
`flow-decompose-project` are NOT monitored because their unique id
prevents cross-invocation collision.

Intermediate content files that the model Writes as input to
`bin/flow write-rule` (for example
`.flow-states/<branch>-dag-content.md`) are also not monitored — they
are the Write-tool input, not a persistent target. The `write-rule`
subcommand reads and deletes them unconditionally.

When a new persistent path becomes a monitored target (e.g. a new skill
writes to a shared file or a new machine-level singleton is introduced),
add it to this list AND to `WRITE_MONITORED_PATHS` in
`tests/skill_contracts.rs`. The contract test scans every SKILL.md for
Write-tool instructions adjacent to any entry in that constant.

## The Write-Rule Escape Pattern

The pattern `flow-learn` uses for `.claude/` writes also applies to all
monitored paths:

1. The model Writes content to `.flow-states/<branch>-<purpose>-content.md`
   using the Write tool. The content file has a unique name per write
   (branch + purpose), so pre-existence is rare.
2. The model invokes `bin/flow write-rule --path <final_target>
   --content-file <content_file>`. The Rust code reads the content file,
   calls `std::fs::write(<final_target>, <content>)` unconditionally,
   and deletes the content file.

Because `std::fs::write` runs inside the `write-rule` subprocess and
never goes through Claude Code's Write tool, the preflight cannot fire
on the final target.

### Intermediate Content File Naming and Lifecycle

Intermediate content files follow the pattern
`.flow-states/<branch>-<purpose>-content.<ext>` where `<purpose>`
matches the basename of the final target (e.g. `dag`, `plan`,
`commit-msg`, `issue-body`, `orchestrate-queue`) and `<ext>` matches the
target's extension (`.md`, `.json`, `.txt`). The `write-rule` subcommand
deletes the intermediate file after a successful routing; on error the
file is left in place so the user can diagnose the routing failure.

Reference implementation: `src/write_rule.rs`.

## The Edit Preamble Pattern

Edit-tool instructions on named `.flow-states/<branch>-*.md` files must
be preceded by an explicit Read-tool instruction on the same file. The
preamble ensures the Edit preflight is satisfied even when the model
has not naturally read the file in the current turn (for example,
re-entering the plan-check fix loop after `--continue-step`).

Canonical wording:

> Use the Read tool on the plan file at `.flow-states/<branch>-plan.md`
> first to satisfy Claude Code's Edit-tool preflight, then use the Edit
> tool to ...

No new subcommand is needed for the Edit case. Edit's `old_string`
requirement forces the model to know the existing content, so a Read
before Edit is already the natural workflow — the preamble just
guarantees it in paths where the natural order could be skipped.

## Enforcement

Two contract tests in `tests/skill_contracts.rs` enforce both sides of
the rule:

- `file_tool_preflight_write_paths_route_through_write_rule` — scans
  every `skills/**/SKILL.md` for Write-tool instructions (matching
  `use`, `using`, `invoke`, `invoking`, `call`, `calling`, `run`,
  `running` followed by `the Write tool`) adjacent to a monitored
  path, and asserts a `bin/flow write-rule --path <same-path>` call
  appears on a SINGLE line within the next 30 lines. Same-line
  co-occurrence is required so a disconnected `bin/flow write-rule`
  targeting a different path plus an unrelated mention of the
  monitored path cannot silently satisfy the check.
- `file_tool_preflight_edit_paths_preceded_by_read` — scans every
  SKILL.md for Edit-tool instructions on named plan or DAG files and
  asserts a Read-tool instruction (matching the same verb vocabulary)
  on the same file appears within the preceding 12 non-blank lines.
  The backward scan stops at any `## ` or `### ` heading so a Read in
  a prior step cannot credit an Edit in a later step — a
  `--continue-step` resume invalidates the prior Read.

Both scans use `write_path_is_bounded` to check BOTH prefix and suffix
byte boundaries on every path match, rejecting longer paths that embed
a monitored path as a substring (e.g. `my-orchestrate-queue.json`,
`.flow-states/<branch>-commit-msg.txt.bak`).

When either test fails, the violation names the file and line. The fix
is to adopt the Write-Rule Escape Pattern or the Edit Preamble Pattern
respectively — never to add an allow-list that exempts the callsite.

## Why Not Skill Instructions Alone

Per `.claude/rules/hook-vs-instruction.md`: when the consequence of
non-compliance is user-visible and blocks the flow, the enforcement
must be mechanical, not advisory. The Write-side fix is mechanical via
the `write-rule` subprocess. The Edit-side fix is advisory prose in
SKILL.md, but the contract test locks the prose invariant in so drift
fails CI.

## Cross-References

- `.claude/rules/hook-vs-instruction.md` — the principle that mandates
  mechanical enforcement for this class.
- `.claude/rules/scope-expansion.md` — the scope-boundary decision for
  combining Write and Edit fixes in one PR.
- `.claude/rules/tests-guard-real-regressions.md` — the discipline
  requiring every test to guard a named regression and name its
  consumer.
- `src/write_rule.rs` — the reference Rust subcommand.
- `skills/flow-learn/SKILL.md` — the reference SKILL.md pattern that
  routes through `write-rule` for CLAUDE.md and `.claude/rules/` writes.
