# Permission Patterns

## Specificity Over Breadth

Use the narrowest pattern that serves the consumer. When the
consumer needs a known file extension, use that extension — never
replace it with a wildcard.

- `Read(//tmp/*.md)` — correct when the consumer reads markdown
- `Read(//tmp/*)` — too broad; covers every file type in `/tmp/`

Directory-level wildcards are acceptable only when every file in
the directory is a valid target. `Read(~/.claude/rules/*)` is fine
because all files in that directory are rules.

## Consumer Traceability

Every allow-list pattern must have a known consumer — a specific
skill, plugin, hook, or tool that needs the permission. If you
cannot name the consumer, do not add the pattern.

Before proposing a new pattern, answer: "Which skill or tool
invokes this command or reads this path?" If the answer is vague
("something might need it"), the pattern is speculative and should
not be added.

## Adding Patterns

When adding a new allow-list pattern, name the consumer in the
commit message or PR description so the audit trail is preserved.

Example commit message:

```text
Add Read(//tmp/*.diff) permission for code-review plugin
```

This makes the allow list auditable — any pattern can be traced
back to why it was added and what breaks if it is removed.

## Plan-Phase Enumeration of Skill-Added Bash Commands

When a plan modifies a skill (`skills/**/SKILL.md` or
`.claude/skills/**/SKILL.md`) to invoke a new bash command, the
plan's Tasks section MUST enumerate the command and confirm one of:

1. The command's first whitespace-separated token already matches
   an existing entry in `UNIVERSAL_ALLOW`
   (`src/prime_check.rs::UNIVERSAL_ALLOW`), so no allow-list change
   is needed.
2. The plan adds the matching `Bash(<pattern>)` entry to BOTH
   `UNIVERSAL_ALLOW` (the canonical Rust source) and
   `skills/flow-prime/SKILL.md` (the prime permissions JSON block —
   the source `tests/permissions.rs::all_bash_commands_have_permission_coverage`
   reads at test time).

Forgetting either side breaks `bin/flow ci`: the contract test in
`tests/permissions.rs` walks every SKILL.md bash block, extracts the
command, and asserts it matches at least one allow-list entry. A
new skill bash command without a matching entry fails CI in Code
phase.

### What counts as a new bash command

A bash block in a SKILL.md introduces a "new" command when its
first token (the program name, modulo `${CLAUDE_PLUGIN_ROOT}/`
prefix) is not already covered by an existing `UNIVERSAL_ALLOW`
entry. Examples:

- A skill that adds `bin/test --adversarial-path` introduces a new
  command — `Bash(bin/test --adversarial-path)` is the matching
  entry.
- A skill that adds `${CLAUDE_PLUGIN_ROOT}/bin/flow custom-subcmd`
  is covered by the existing `Bash(*bin/flow *)` entry — no
  permission change needed.
- A skill that adds `gh release upload <tag> <file>` likely needs
  a new `Bash(gh release upload *)` entry.

### How to apply

**Plan phase.** For every plan task that modifies a SKILL.md to
add a bash block, the task description must include a "Permission
coverage" subsection naming:

1. The command's first token.
2. The matching existing `UNIVERSAL_ALLOW` entry, OR the new
   `Bash(<pattern>)` entry the plan will add to both
   `src/prime_check.rs::UNIVERSAL_ALLOW` and
   `skills/flow-prime/SKILL.md`.
3. An acknowledgement that adding to `UNIVERSAL_ALLOW` will bump
   `compute_config_hash`, requiring a `CURRENT_CONFIG_HASH` update
   in `tests/prime_check.rs::compute_config_hash_uses_python_default_formatter`.

**Code phase.** When implementing the SKILL.md change, the same
commit must include the matching allow-list addition + the
`CURRENT_CONFIG_HASH` bump.

**Code Review phase.** The reviewer agent cross-checks every new
SKILL.md bash command in the diff against the diff's allow-list
changes. A SKILL.md bash command without a matching allow-list
entry is a Real finding fixed in Step 4.

## Never Remove Without Explicit Ask

When editing `.claude/settings.json`, only add entries — never
remove existing permission entries unless the user explicitly asks.
An entry may serve purposes the current task does not know about.

When an entry needs to be repositioned, add first in the new
location, then remove the duplicate — and explain the two-step
approach before starting.

### Prime-Time Active Deny Removal Carve-Out

`/flow:flow-prime` runs `merge_settings` during initial setup and
re-prime. The merge enforces an "allow always wins" invariant:
when an entry's exact string appears in BOTH the existing allow
list AND the existing deny list, the deny entry is removed
during the merge. The same exact-string match also blocks FLOW's
own `FLOW_DENY` entries from being appended when the user has
already opted into the same permission via allow.

This is the one sanctioned exception to "never remove without
explicit ask" — the user implicitly asks for it by running
`/flow:flow-prime`, and the action targets only entries the user
themselves placed in conflicting lists. The merge does not
remove deny entries that have no allow-list counterpart, and
`UNIVERSAL_ALLOW` / `FLOW_DENY` are validated against each other
by the `no_allow_deny_overlap_in_plugin_permissions` test.

The match is exact-string only. A user with `Bash(git push)` in
their deny list and `Bash(git *)` (broader pattern) in their
allow list keeps the deny — subsumption-based removal is out of
scope. See `src/prime_setup.rs::merge_settings_with`.

## Never Edit Permissions Mid-Flow

Never modify `.claude/settings.json` inside a worktree during an
active FLOW phase. Claude Code enforces permission changes
immediately — removing or narrowing a pattern breaks tools the
current task still needs, causing permission prompts or hook
blocks mid-session.

Permission lockdown changes belong in `src/prime_check.rs`
(UNIVERSAL_ALLOW, FLOW_DENY) for target projects. The FLOW repo's
own `.claude/settings.json` is updated on the base branch after
the PR merges, or during the next `/flow:flow-prime` run.

### Mechanical enforcement at the subprocess layer

`src/promote_permissions.rs::active_flow_gate` blocks
`bin/flow promote-permissions` mid-flow. Before the merge runs, it
walks up the resolved `--worktree-path` looking for
`.flow-states/`, derives the branch via
`crate::hooks::detect_branch_from_path`, and checks
`crate::hooks::is_flow_active(&branch, &main_root)`. When a flow
is active and the caller did NOT pass `--confirm-on-flow-branch`,
the gate returns:

```json
{"status": "skipped", "reason": "active_flow",
 "message": "...", "branch": "<branch>"}
```

The local settings file is preserved across the skip so a
confirmed retry (typically from `flow-learn` Step 4, the only
sanctioned mid-flow caller) completes the merge.

The `--confirm-on-flow-branch` flag is the bypass. `flow-learn`
Step 4 passes it; any other caller documenting a legitimate
mid-flow promotion must do the same.

## Shared Config Files — Express User Permission Required

Some files in the worktree are not FLOW state and not task-scoped
code — they are shared configuration that affects every engineer
working in the repository. These files must not be modified during
an active FLOW phase without explicit user permission, even when
the change would simplify the current task.

The canonical list:

- `.gitignore` / `.gitattributes` — affect every git operation
  across all engineers on the branch
- `Makefile`, `Rakefile`, `justfile`, `package.json`,
  `requirements.txt`, `go.mod`, `Cargo.toml` — shared build and
  dependency config (modifications may churn lockfiles and shift
  versions under other engineers' feet)
- `.github/` (workflows, issue templates, CODEOWNERS) — affect
  every PR in the repo
- `.config/` (everything under it — `nextest.toml`, build profile
  configs, language-toolchain configs, etc.) — shared
  build/test infrastructure that every engineer's CI run inherits
- `.claude/settings.json` — covered by "Never Edit Permissions
  Mid-Flow" above

When a PR's scope is narrow (e.g., "fix one flaky test"), editing
any of these files expands the diff into territory the user never
agreed to review.

## The Correct Path

When a task's natural cleanup requires modifying a shared config
file, stop and ask the user:

> "The cleanest solution here requires adding one line to
> `.gitignore` (or modifying `.github/workflows/ci.yml`,
> `.config/nextest.toml`, etc.). This is shared config that
> every engineer reads. May I modify it, or should I change the
> approach to avoid the edit?"

Prefer approaches that keep the diff scoped to task-relevant
code. Ask before expanding scope into shared territory. If the
user approves the edit, proceed. If not, find a different path.

## Enforcement

Shared-config protection is a workflow discipline, not a universal
rule. Outside a flow context, users can modify shared config
freely. Once a flow starts and the session is inside a worktree,
the gate activates to enforce the explicit-permission requirement.

The `validate-worktree-paths` PreToolUse hook
(`src/hooks/validate_worktree_paths.rs`) enforces this rule
mechanically. The `is_shared_config` predicate matches the nine
canonical filenames (`.gitignore`, `.gitattributes`, `Makefile`,
`Rakefile`, `justfile`, `package.json`, `requirements.txt`,
`go.mod`, `Cargo.toml`) plus any path passing through a `.github/`
directory component.

**Coverage gap.** The hook does NOT yet match `.config/` paths
(e.g., `.config/nextest.toml`). The prose rule above forbids
unapproved edits to those files; the hook does not yet enforce
them. Until the hook is extended, the model must follow the prose
rule manually for `.config/` writes.

The `validate_shared_config` function gates on tool name: only
`Edit` and `Write` tool calls are blocked (exit 2). `Read`,
`Glob`, and `Grep` calls pass through. The block fires only when
the CWD is inside a `.worktrees/` directory and the target path
is inside the worktree.

The block message directs the model to confirm with the user via
`AskUserQuestion` before proceeding, and points to this section
for context.
