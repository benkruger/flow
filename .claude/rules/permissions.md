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
phase. Without Plan-time enumeration, the gap surfaces as a
mid-Code permission failure that triggers an unplanned plan
deviation log + a `CURRENT_CONFIG_HASH` bump (the allow-list
addition changes the config-hash inputs).

### What counts as a new bash command

A bash block in a SKILL.md introduces a "new" command when its
first token (the program name, modulo `${CLAUDE_PLUGIN_ROOT}/`
prefix) is not already covered by an existing `UNIVERSAL_ALLOW`
entry under the project's permission-pattern matching. Examples:

- A skill that adds `bin/test --adversarial-path` introduces a new
  command — `Bash(bin/test --adversarial-path)` is the matching
  entry (or `Bash(bin/test *)` if the project wants to broaden).
- A skill that adds `${CLAUDE_PLUGIN_ROOT}/bin/flow custom-subcmd`
  is covered by the existing `Bash(*bin/flow *)` entry — no
  permission change needed.
- A skill that adds `gh release upload <tag> <file>` is covered by
  existing `Bash(gh release create *)` only if patterns subsume —
  most likely a new `Bash(gh release upload *)` entry is needed.

### How to apply

**Plan phase.** For every plan task that modifies a SKILL.md to
add a bash block, the task description must include a "Permission
coverage" subsection naming:

1. The command's first token (e.g., `bin/test --adversarial-path`,
   `${CLAUDE_PLUGIN_ROOT}/bin/flow <new-subcmd>`).
2. The matching existing `UNIVERSAL_ALLOW` entry, OR the new
   `Bash(<pattern>)` entry the plan will add to both
   `src/prime_check.rs::UNIVERSAL_ALLOW` and
   `skills/flow-prime/SKILL.md`.
3. An acknowledgement that adding to `UNIVERSAL_ALLOW` will bump
   `compute_config_hash`, requiring a `CURRENT_CONFIG_HASH` update
   in `tests/prime_check.rs::compute_config_hash_uses_python_default_formatter`.

**Code phase.** When implementing the SKILL.md change, the same
commit must include the matching allow-list addition + the
`CURRENT_CONFIG_HASH` bump. Discovering the gap mid-Code is a
plan-deviation event per
`.claude/rules/plan-commit-atomicity.md` and must be logged via
`bin/flow log` before the commit lands.

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
by the `no_allow_deny_overlap_in_plugin_permissions` test so
FLOW never engineers a conflict that would silently strip a
user's deny.

The match is exact-string only. A user with `Bash(git push)` in
their deny list and `Bash(git *)` (broader pattern) in their
allow list keeps the deny — subsumption-based removal is out of
scope. See `src/prime_setup.rs::merge_settings_with` for the
implementation and the doc comment that records the contract.

## Never Edit Permissions Mid-Flow

Never modify `.claude/settings.json` inside a worktree during an
active FLOW phase. Claude Code enforces permission changes
immediately — removing or narrowing a pattern breaks tools the
current task still needs, causing permission prompts or hook
blocks mid-session.

Permission lockdown changes belong in `src/prime_check.rs`
(UNIVERSAL_ALLOW, FLOW_DENY) for target projects. The FLOW repo's
own `.claude/settings.json` is updated on main after the PR merges,
or during the next `/flow:flow-prime` run.

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
  build/test infrastructure that every engineer's CI run inherits.
  A change to `.config/nextest.toml` (test timeouts, test groups,
  parallelism limits) reshapes every concurrent flow's CI behavior.
- `.claude/settings.json` — covered by "Never Edit Permissions
  Mid-Flow" above

When a PR's scope is narrow (e.g., "fix one flaky test"), editing
any of these files expands the diff into territory the user never
agreed to review. Even a one-line change to `.gitignore` or
`.config/nextest.toml` is a scope expansion — the user has not
seen or approved that entry.

## The Anti-Pattern

The motivating incident (PR #1166): Claude created a helper
script `.flow-loop-runner.sh` whose execution was blocked by the
permission model. To keep the orphan file out of the commit,
Claude added the filename to `.gitignore` without user permission.
The user had to revert the `.gitignore` change manually after
catching it. Two violations compounded: the script never should
have been created (see
`.claude/rules/permission-blocked-workarounds.md`), and
`.gitignore` never should have been modified to work around the
script.

A second motivating incident: a CI test under nextest's
`slow-timeout` was failing under heavy machine load (video call).
Claude attempted to add a serial test-group override in
`.config/nextest.toml` to "fix" the flake. That was scope
expansion into shared infrastructure (every engineer's CI
inherits the override) AND it was solving an environmental noise
problem with a config change rather than waiting for load to
return to normal. The user reverted both. See also
`.claude/rules/testing-gotchas.md` "Distinguish Environmental
Load From Flaky Tests" for the test-side discipline.

## The Correct Path

When a task's natural cleanup requires modifying a shared config
file, stop and ask the user:

> "The cleanest solution here requires adding one line to
> `.gitignore` (or modifying `.github/workflows/ci.yml`,
> `.config/nextest.toml`, etc.).
> This is shared config that every engineer reads. May I modify
> it, or should I change the approach to avoid the edit?"

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
rule manually for `.config/` writes — every Edit/Write to a
`.config/*` file under an active flow requires explicit user
approval before the call. Future work is to extend the
`is_shared_config` predicate to match any path passing through a
`.config/` directory component, mirroring the existing
`.github/` treatment.

The `validate_shared_config` function gates on tool name: only
`Edit` and `Write` tool calls are blocked (exit 2). `Read`,
`Glob`, and `Grep` calls pass through so codebase exploration is
unaffected. The block fires only when the CWD is inside a
`.worktrees/` directory (the flow-active proxy) and the target
path is inside the worktree.

The block message directs the model to confirm with the user via
`AskUserQuestion` before proceeding, and points to this section
for context. No hook registration changes are needed — the
existing `validate-worktree-paths` entries for Edit and Write in
`hooks/hooks.json` already cover the tool surface.

## Cross-References

- `.claude/rules/permission-blocked-workarounds.md` — documents the
  first half of the compound anti-pattern (creating the orphan
  artifact) that commonly motivates shared-config modification.
- `.claude/rules/code-review-scope.md` "Rules Landed on Main
  Mid-Flow" — covers the adjacent case of shared rules updated
  on main during an active branch.
- `.claude/rules/testing-gotchas.md` "Distinguish Environmental
  Load From Flaky Tests" — the test-side discipline that
  prevents shared-config edits from being used to paper over
  environmental load events.
- `.claude/rules/cli-output-contracts.md` — when a new skill
  bash command is also a new consumed-output subcommand or stub
  flag, the Plan-phase enumeration here couples with the
  output-contract discipline there. The plan must specify both
  the permission entry AND the output contract before Code phase.
