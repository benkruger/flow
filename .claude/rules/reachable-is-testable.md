# Production-Reachable Is Testable

If a line of code runs when a user invokes the public interface,
a test driving the same interface can reach it. "Untestable" is
never a terminal classification — it is always shorthand for one
of three states, and which one must be named before any action
is taken.

## The triage

When a line resists coverage, work the three questions in order:

1. **Is this line reachable in production at all?** Grep the
   callers of the enclosing function. Trace back to the public
   surface (a `bin/flow` subcommand, a hook entry point, a skill
   invocation). If nothing production reaches it, the line is
   dead — delete it. No test to write.

2. **If it is reachable, what does the user's environment supply
   that the test's environment does not?** Name the specific
   difference: a real TTY, a populated `$HOME`, a specific git
   state, a running subprocess, a particular filesystem layout.
   The name of the missing piece is the fixture to build.

3. **Is the test driving the same public entry point the user
   drives?** If the test calls a private helper but the user
   calls the outer subcommand, the test is wrong. Rewrite the
   test to invoke the outer entry through a subprocess or the
   library's public function — whichever matches the production
   path.

Only after (1)–(3) resolve does `testability-means-simplicity.md`
apply. Simplification is the response when the triage surfaces
an over-engineered branch with no legitimate public consumer,
not the first instinct when a test is hard to write.

## Terminal states

A coverage investigation ends in exactly one of:

- the line is covered by a test,
- the line is deleted because no production path reaches it,
- an explicit open question to the user naming which fixture
  piece is missing and asking whether to build it or refactor
  the production path around it.

Reporting "<100%, blocked" or "<100%, environment-limited" as a
completion state is a failure to apply the triage. The three
questions above always produce a concrete next action.

### "Covered elsewhere" is not a terminal state

Asserting that an uncovered line in the per-file gate is "covered
elsewhere" by tests in another binary — without verifying the
claim against the full `bin/flow ci` 100/100/100 result — is a
fourth invalid completion state. The per-file gate
(`bin/test tests/<path>/<name>.rs`) compiles a single test binary
and only sees coverage from that binary's tests; it does not see
contributions from `tests/hooks.rs`, `tests/main_dispatch.rs`, or
any other binary. A line uncovered in the per-file report may
still hit 100% in the full-CI aggregate — but only if a specific
test in another binary actually exercises it.

The required investigation:

1. **Name the test that covers the line.** If you assert "covered
   elsewhere," you must name the specific test function and its
   binary (e.g.,
   `tests/hooks.rs::validate_worktree_paths_shared_config_edit_gitignore_blocked`).
2. **Verify the full-CI aggregate.** Run `bin/flow ci` end-to-end
   and confirm the TOTAL row reads `100.00%` for regions,
   functions, and lines. The aggregate is the gate; the per-file
   number is the diagnostic.
3. **Only then proceed.** A "covered elsewhere" assertion without
   the named test AND the verified aggregate is not a completion
   state — it is speculation, equivalent to "<100%, but I think
   it's fine."

If a line is genuinely uncovered in the aggregate, return to the
three triage questions. The discipline forbids reporting any
form of partial coverage as final; "covered elsewhere" is the
specific phrasing that tries to slip past this discipline by
displacing the responsibility to a test the speaker has not named
or verified.

## Fixture recipes for the common hard cases

The seam-injection carve-out in `rust-patterns.md` names the
externally-coupled resources that justify a `pub` test seam
(real TTY, raw-mode terminal, live crossterm event loop, network
socket). Those seams still need a production-binding test — the
fixture shapes below drive them:

- **Real TTY / controlling terminal**: `libc::openpty` +
  `pre_exec` running `setsid()`, `ioctl(TIOCSCTTY)`, and `dup2`
  of the slave onto fds 0/1/2 before `execvp`. The parent writes
  to the master fd to send keystrokes. The `portable-pty` crate
  wraps the same sequence at a higher level.
- **`env::current_dir()` returns `Err`**: `pre_exec` running
  `libc::rmdir` on the child's cwd after the kernel's `chdir`
  but before `execvp`. The child's subsequent `getcwd()` returns
  `ENOENT`.
- **`fs::read_to_string` returns `Err` on an existing file**:
  `chmod 000` on the file; restore in test teardown so the
  `TempDir` drop cleans up.
- **`Command::new` returns `Err` on spawn**: isolate the
  child's `PATH` to an empty string or a directory without the
  binary. When the module under test also calls other binaries
  that must succeed first, supply a directory with a targeted
  shim script that `exit 0`s for the prerequisites and returns
  a spawn-failing name for the target.
- **Stdin read fails**: `pre_exec` closing fd 0 before `execvp`,
  or piping `/dev/null` and closing the parent end immediately.

Adding a fixture class to this list as new hard cases surface
is part of following this rule, not a deviation from it.

## How to apply

**When a coverage gap surfaces.** Work the three triage
questions in order before editing any file. Write the answer
somewhere durable — plan notes, commit body, or inline in the
conversation with the user. The fix that follows from the named
answer is the fix that is allowed.

**When reporting status.** A partial-coverage number is never
the last word. The only valid reports are (a) 100%, (b) a line
deleted with the reason, or (c) an explicit question naming
which fixture piece needs a decision. "I hit a limit" is not a
report — it is a request for help that must be phrased as a
question. "Covered elsewhere" without a named test and a verified
aggregate is also not a report — it is speculation.

**When reviewing.** A PR description or commit body that
asserts a line is "hard to test" without naming which of the
three states applies is an incomplete review. Ask which state
before approving any workaround. A claim of "covered elsewhere"
must cite the specific test function and binary, and the reviewer
should confirm the full-CI aggregate reads 100/100/100 before
accepting.

## Cross-references

- `.claude/rules/testability-means-simplicity.md` — the response
  when the triage surfaces an over-engineered branch.
- `.claude/rules/no-waivers.md` — the 100/100/100 gate this rule
  protects.
- `.claude/rules/per-file-coverage-iteration.md` — the per-file
  gate is for iteration speed, not for completion reporting; the
  full-CI gate is the authority.
- `.claude/rules/rust-patterns.md` "Seam-injection variant for
  externally-coupled code" — the seam patterns whose production
  bindings the fixture recipes above test.
- `.claude/rules/test-placement.md` — every test lives in
  `tests/<path>/<name>.rs` and drives through the public
  interface. The triage above is how that discipline stays
  honest when the public path is fixture-hungry.
