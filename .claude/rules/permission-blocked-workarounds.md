# Permission-Blocked Workarounds

When the permission model (allow/deny lists in
`.claude/settings.json`, plus the global `validate-pretool` hook)
blocks an operation, never create a new artifact as a workaround.
In particular: never write a helper script to batch operations
that the Bash allow list forbids. A script that cannot be
executed via the permission model is almost always also
impossible to delete via the permission model, leaving an orphan
artifact in the worktree.

## The Pattern

The temptation: Claude needs to run a Bash command N times. The
allow list forbids compound commands, shell loops, and arbitrary
script invocations. To batch the work, Claude writes a helper
script and tries to invoke it. The invocation is blocked. The
cleanup `rm` is also blocked. The file now sits in the worktree.

To keep the orphan out of the commit, Claude may then modify a
shared config file (typically `.gitignore`) without user permission
— a second scope-expansion violation (see
`.claude/rules/permissions.md` "Shared Config Files").

## The Correct Path

When you need N sequential or parallel operations and the
permission model blocks the obvious shell idiom:

1. **Fire N Bash tool calls directly.** The Bash tool itself
   accepts individual commands that are allow-listed. Ten
   sequential Bash calls in ten separate responses (or grouped
   in parallel batches) work within the permission model and
   produce no orphan artifacts. Overhead is real but capped.
2. **Stop and ask the user.** Say: "I need to run X ten times.
   A helper script would be cleaner but the permission model
   blocks both the invocation and the cleanup. Want me to (a)
   fire ten Bash calls sequentially, (b) expand the allow list
   for a single specific script in this worktree, or (c) change
   the approach entirely?"
3. **Never create the orphan artifact.** Do not write a `.sh`,
   `.py`, `.rb`, or any other script file as a "temporary"
   workaround during an active FLOW phase. Temporary files
   without a cleanup path are not temporary.

## Why

The permission model is not an obstacle to work around — it is
a deliberate narrowing of the action surface that the user has
reviewed and approved. Creating artifacts to bypass the model
defeats the review. Worse, orphan artifacts force scope
expansion (`.gitignore` entries, manual cleanup requests) that
further dilutes the user's review.

## Sanctioned Bypass: User-Approved Edit via `bin/flow write-rule`

This rule prohibits *creating* workaround artifacts. It does NOT
prohibit *using* sanctioned system tools that exist precisely to
mediate hook-blocked operations. The distinguishing test:

- **Workaround artifact (forbidden):** A new `.sh`/`.py`/`.rb`
  helper, a hand-rolled script, a manually-constructed bypass
  binary, or any other artifact the model creates to do work
  the permission model blocks. The permission model rejects the
  artifact's invocation AND its cleanup, leaving an orphan.
- **Sanctioned bypass (permitted under conditions):** Invoking
  `bin/flow write-rule` — a project-supplied CLI subcommand
  designed for `.claude/`-path edits and managed-artifact
  routing — to apply a write that the Edit/Write tool is
  blocking. `write-rule` is not gated by `validate-worktree-paths`
  because it's a Bash invocation, not an Edit/Write tool call.
  Its existence is documented in
  `.claude/rules/file-tool-preflights.md` "The Write-Rule
  Escape Pattern" as the canonical mechanism for these writes.

The shared-config carve-out specifically: when
`validate-worktree-paths` blocks an Edit/Write on
`Cargo.toml`/`.gitignore`/`.github/`/etc., the hook's instruction
("Use `AskUserQuestion` to confirm") is the path the rule
expects. When `AskUserQuestion` succeeds and the user explicitly
approves the edit, the hook's *intent* (user approval) is
satisfied even though its *mechanism* (Edit/Write blocking) is
still active. In this state, applying the user-approved edit via
`bin/flow write-rule` is a sanctioned bypass — the rule's intent
is preserved while a mechanism gap is closed.

The full conditions for the sanctioned bypass:

1. **The user has explicitly approved the specific edit** via
   `AskUserQuestion` in the same session, and the approval is
   logged via `bin/flow log` so the audit trail records that
   the user-approval requirement was satisfied.
2. **The bypass uses `bin/flow write-rule`** — not a hand-rolled
   `sed -i`, `cat > file`, `tee`, or any other shell idiom that
   would qualify as a workaround artifact under the rule above.
3. **The bypass applies the exact edit the user approved** —
   not a related edit, not an extended scope. If the
   AskUserQuestion approved deleting 8 stanzas, `write-rule`
   applies a Cargo.toml content with exactly those 8 stanzas
   removed.

Without all three conditions, this carve-out does not apply and
the rule's "never create a new artifact as a workaround"
prohibition stands.

## Cross-References

- `.claude/rules/permissions.md` "Shared Config Files — Express
  User Permission Required" documents the second half of the
  anti-pattern (modifying `.gitignore` etc. to hide the orphan).
- `.claude/rules/file-tool-preflights.md` "The Write-Rule
  Escape Pattern" documents `bin/flow write-rule` as the
  canonical mediator for hook-blocked path writes.
- `.claude/rules/ci-is-a-gate.md` documents the related rule
  that `bin/flow` subcommands must never run in the background.
