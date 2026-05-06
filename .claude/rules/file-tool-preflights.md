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
workflow.

## Monitored Target Paths

Monitored paths are files FLOW skills write to repeatedly across
invocations (branch-scoped or machine-level singletons) or files that
may pre-exist from a prior session on fresh re-entry. Session-scoped
files with a unique `-<id>` suffix are excluded because the id makes
cross-invocation collision unlikely. Writes to the monitored set must
route through `bin/flow write-rule`:

- `.flow-states/<branch>/dag.md` — the decompose-produced DAG file
- `.flow-states/<branch>/plan.md` — the Plan-phase implementation plan
- `.flow-states/<branch>/commit-msg.txt` — the commit skill's message
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
`.flow-states/<branch>/dag-content.md`) are also not monitored — they
are the Write-tool input, not a persistent target. The `write-rule`
subcommand reads and deletes them unconditionally.

When a new persistent path becomes a monitored target, add it to this
list AND to `WRITE_MONITORED_PATHS` in `tests/skill_contracts.rs`. The
contract test scans every SKILL.md for Write-tool instructions adjacent
to any entry in that constant.

## Canonical Location for `.flow-states/`

The `.flow-states/` directory is the shared state surface for FLOW
flows and lives ONLY at `<project_root>/.flow-states/`. Tool-call
writes to `<project_root>/.worktrees/<branch>/.flow-states/...` (the
worktree-internal copy at the worktree root) or
`<project_root>/.worktrees/<branch>/<service>/.flow-states/...` (the
mono-repo service-subdirectory variant) would create a misplaced state
copy that the readers (cleanup, discovery scanners, hooks) cannot see.

The `validate-worktree-paths` PreToolUse hook
(`src/hooks/validate_worktree_paths.rs`) enforces the canonical
location at the tool-call layer. The
`detect_misplaced_flow_states(file_path, project_root)` helper detects
the misplacement via pure string operations and returns the canonical
destination. When the helper matches, `validate()` returns
`(false, message)` with a `BLOCKED` message naming the canonical path
the caller should use instead.

The check fires on every Edit, Write, Read, Glob, and Grep tool call
the hook is registered for. Both `file_path` (Edit/Write/Read) and
`path` (Glob/Grep) input shapes resolve through `get_file_path`
before the helper runs.

## Managed-Artifact Canonicalization Gate (CLI Layer)

The hook above closes the tool-call surface; `bin/flow write-rule`
itself closes the CLI surface. When `--path` is invoked with a basename
that names a FLOW-managed artifact, write-rule rejects any path that
isn't the canonical destination computed from
`(project_root, current_branch)` via `FlowPaths`.

**The managed-artifact set.** Five basenames are managed; every other
basename passes through unchanged:

| Basename | Variant | Canonical destination |
|---|---|---|
| `plan.md` | `PlanMd` | `<project_root>/.flow-states/<branch>/plan.md` |
| `dag.md` | `DagMd` | `<project_root>/.flow-states/<branch>/dag.md` |
| `commit-msg.txt` | `CommitMsgTxt` | `<project_root>/.flow-states/<branch>/commit-msg.txt` |
| `.flow-issue-body` | `FlowIssueBody` | `<project_root>/.flow-issue-body` |
| `orchestrate-queue.json` | `OrchestrateQueue` | `<project_root>/.flow-states/orchestrate-queue.json` |

**The canonicalization rule.** `run_impl_main` calls
`classify_path(args.path)` to look up the variant by basename. When a
variant matches, it computes the canonical destination via
`canonical_path(art, &project_root(), current_branch().as_deref())`,
resolves the provided path to absolute (relative paths join against
`project_root`), lexically normalizes both sides (resolving `..`
segments), and rejects when the two normalized PathBufs differ.

**Ordering invariants.** The gate runs BEFORE `read_content_file` so
a rejection does not destroy the caller's input file. When the gate
accepts, the actual `fs::write` call uses the resolved absolute path —
never the original `args.path` — so a relative `--path` validated
against `project_root` cannot be silently re-resolved by `fs::write`
against the process cwd at write time.

**The error shape.** A rejection returns JSON to stdout and exits 1:

```json
{
  "status": "error",
  "step": "path_canonicalization",
  "message": "write-rule rejects --path <provided> for managed artifact <art>: canonical destination is <canonical>",
  "provided": "<provided>",
  "canonical": "<canonical>",
  "artifact_kind": "PlanMd|DagMd|CommitMsgTxt|FlowIssueBody|OrchestrateQueue"
}
```

**Pass-through for non-managed paths.** When the basename isn't in
the set above (e.g., `.claude/rules/<topic>.md`, `CLAUDE.md`,
arbitrary user-named files), the gate is silent and write-rule writes
the path verbatim. This is the path the `flow-learn` rule-routing
pattern depends on.

**Pass-through for branch-unavailable contexts.** Branch-scoped
artifacts (`PlanMd`, `DagMd`, `CommitMsgTxt`) require a valid
non-empty branch. In detached-HEAD or invalid-branch (slash-
containing) contexts, `canonical_path` returns `None` and the gate
stays silent.

The gate complements the tool-call-layer hook above: write-rule
canonicalizes its `--path` argument before `fs::write`, and the
`validate-worktree-paths` hook canonicalizes the tool-call layer
before either Write or write-rule runs.

## The Write-Rule Escape Pattern

The pattern `flow-learn` uses for `.claude/` writes also applies to all
monitored paths:

1. The model Writes content to `.flow-states/<branch>/<purpose>-content.md`
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
`.flow-states/<branch>/<purpose>-content.<ext>` where `<purpose>`
matches the basename of the final target (e.g. `dag`, `plan`,
`commit-msg`, `issue-body`, `orchestrate-queue`) and `<ext>` matches the
target's extension (`.md`, `.json`, `.txt`). The `write-rule` subcommand
deletes the intermediate file after a successful routing; on error the
file is left in place so the user can diagnose the routing failure.

Reference implementation: `src/write_rule.rs`.

## The Edit Preamble Pattern

Edit-tool instructions on named `.flow-states/<branch>/*.md` files must
be preceded by an explicit Read-tool instruction on the same file. The
preamble ensures the Edit preflight is satisfied even when the model
has not naturally read the file in the current turn.

Canonical wording:

> Use the Read tool on the plan file at `.flow-states/<branch>/plan.md`
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
  every `skills/**/SKILL.md` for Write-tool instructions adjacent to
  a monitored path, and asserts a `bin/flow write-rule --path <same-path>`
  call appears on a SINGLE line within the next 30 lines.
- `file_tool_preflight_edit_paths_preceded_by_read` — scans every
  SKILL.md for Edit-tool instructions on named plan or DAG files and
  asserts a Read-tool instruction on the same file appears within the
  preceding 12 non-blank lines. The backward scan stops at any `## ` or
  `### ` heading so a Read in a prior step cannot credit an Edit in a
  later step — a `--continue-step` resume invalidates the prior Read.

Both scans use `write_path_is_bounded` to check BOTH prefix and suffix
byte boundaries on every path match, rejecting longer paths that embed
a monitored path as a substring.

The CLI-layer canonicalization gate is enforced by the
`tests/write_rule.rs` subprocess matrix: each managed artifact has a
canonical-success cell and a worktree-misroute-reject cell, plus the
non-managed pass-through and detached-HEAD pass-through cells.

When either contract test fails, the violation names the file and line.
The fix is to adopt the Write-Rule Escape Pattern or the Edit Preamble
Pattern respectively — never to add an allow-list that exempts the
callsite.

## Why Not Skill Instructions Alone

Per `.claude/rules/hook-vs-instruction.md`: when the consequence of
non-compliance is user-visible and blocks the flow, the enforcement
must be mechanical, not advisory. The Write-side fix is mechanical via
the `write-rule` subprocess. The Edit-side fix is advisory prose in
SKILL.md, but the contract test locks the prose invariant in so drift
fails CI.
