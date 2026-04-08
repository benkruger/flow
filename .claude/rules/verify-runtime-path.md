# Verify the Runtime Path

Before writing a fix, trace the actual execution path to confirm
where the code runs.

## Required Steps

1. **Find the real call site.** Grep for all callers. A function
   may exist in one file but be called from another — or not called
   at all if a different code path runs first.
2. **Verify runtime behavior.** Write a small diagnostic script
   that runs through the same call chain (Claude Code → bash →
   bin/flow → flow-rs) and print the actual values. Unit test
   mocks do not catch environment issues like missing ttys,
   wrong parents, or piped stdin.
3. **Check one layer deeper.** When a subprocess returns an
   unexpected value (`??`, empty string, wrong PID), investigate
   why before filtering it out. The wrong value is a symptom.

## Anti-Patterns

- Committing a fix without running it through the real path
- Adding a second fix on top of an unverified first fix
- Trusting unit tests as proof that runtime behavior is correct
  when the bug is environmental (process tree, tty, file system)
- Assuming which file creates/owns a piece of state without
  grepping for all writers
