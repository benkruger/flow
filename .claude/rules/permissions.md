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

## Never Remove Without Explicit Ask

When editing `.claude/settings.json`, only add entries — never
remove existing permission entries unless the user explicitly asks.
An entry may serve purposes the current task does not know about.

When an entry needs to be repositioned, add first in the new
location, then remove the duplicate — and explain the two-step
approach before starting.

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
- `.claude/settings.json` — covered by "Never Edit Permissions
  Mid-Flow" above

When a PR's scope is narrow (e.g., "fix one flaky test"), editing
any of these files expands the diff into territory the user never
agreed to review. Even a one-line change to `.gitignore` is a
scope expansion — the user has not seen or approved that entry.

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

## The Correct Path

When a task's natural cleanup requires modifying a shared config
file, stop and ask the user:

> "The cleanest solution here requires adding one line to
> `.gitignore` (or modifying `.github/workflows/ci.yml`, etc.).
> This is shared config that every engineer reads. May I modify
> it, or should I change the approach to avoid the edit?"

Prefer approaches that keep the diff scoped to task-relevant
code. Ask before expanding scope into shared territory. If the
user approves the edit, proceed. If not, find a different path.

## Enforcement

A proposed `PreToolUse` hook would match `Edit`/`Write` calls
targeting shared config files during an active FLOW phase and
warn with a pointer to this section. Until the hook lands, the
rule file is the primary instrument: every FLOW session must
read it before editing any file in the canonical list above.

## Cross-References

- `.claude/rules/permission-blocked-workarounds.md` — documents the
  first half of the compound anti-pattern (creating the orphan
  artifact) that commonly motivates shared-config modification.
- `.claude/rules/code-review-scope.md` "Rules Landed on Main
  Mid-Flow" — covers the adjacent case of shared rules updated
  on main during an active branch.
