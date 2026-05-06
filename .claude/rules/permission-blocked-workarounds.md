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

## Cross-References

- `.claude/rules/permissions.md` "Shared Config Files — Express
  User Permission Required" documents the second half of the
  anti-pattern (modifying `.gitignore` etc. to hide the orphan).
- `.claude/rules/ci-is-a-gate.md` documents the related rule
  that `bin/flow` subcommands must never run in the background.
