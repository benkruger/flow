# Subprocess Test Hygiene

When a test spawns a subprocess — especially the project's own
compiled binary via `Command::new(env!("CARGO_BIN_EXE_<bin>"))` — the
child inherits the parent's environment by default. Without explicit
env neutralization, the child can reach external services, leak
coverage artifacts, or block on network timeouts. Every subprocess
test must deliberately neutralize the environment surfaces its
subject code might read.

## Why

A subprocess test's purpose is to exercise one specific path through
the binary. Anything the child does beyond that path is pollution:

- **External I/O** — a child that inherits `GH_TOKEN` and shells out
  to `gh` makes a real GitHub API call. In CI environments without
  credentials, the child hangs on network timeout; in developer
  environments with credentials, the child mutates live GitHub
  state (creates labels, closes issues, opens PRs).
- **Coverage artifact leaks** — a child that inherits
  `LLVM_PROFILE_FILE` pointing to a path it cannot resolve writes
  profraw files to its cwd.
- **Ambient config** — a child that inherits `HOME` can read
  `~/.gitconfig`, `~/.cargo/config.toml`, `~/.config/gh/*`, and
  dozens of other dotfiles that vary by engineer, introducing
  hard-to-reproduce test flakiness.

The default "inherit everything" is wrong for tests. The correct
baseline is "inherit nothing the test does not explicitly approve."

## The Rule

Every test that spawns the project's binary via
`Command::new(env!("CARGO_BIN_EXE_<bin>"))` — or any other
`Command::new` targeting a process that reads the ambient
environment — must explicitly neutralize these surfaces:

1. **Network credential vars** for any service the subject code
   might talk to:
   - GitHub: `GH_TOKEN`, `GITHUB_TOKEN` — set to `"invalid"` so
     `gh` fails auth fast rather than hanging on network
   - Slack: `SLACK_WEBHOOK_URL`, `SLACK_BOT_TOKEN`,
     `SLACK_CHANNEL` — set to empty or `env_remove`
   - AWS / GCP / Azure — whichever cloud's SDK the subject
     uses: `env_remove` the relevant credential vars
2. **Ambient config homes**:
   - `HOME` — set to the test's canonical tempdir root so the
     child reads no user dotfiles
3. **Recursion guards** — project-specific env vars that the
   binary uses to detect re-entry. For FLOW:
   - `FLOW_CI_RUNNING` — `env_remove` if the test invokes a
     CI-tier subcommand that would refuse to run with the guard
     set
4. **Coverage artifact controls** — `LLVM_PROFILE_FILE` is
   normally safe to inherit (parent's cargo-llvm-cov sets it to
   a valid template), but tests that change cwd to a tempdir
   that lacks `target/llvm-cov-target/` risk the child falling
   back to `default_*.profraw` in cwd. Either:
   - Set `current_dir(worktree_root)` so the inherited template
     resolves correctly, OR
   - Rely on the repo-level safety net (the `.gitignore`
     `*.profraw` pattern plus `bin/test`'s `default_*.profraw`
     sweep)

## Machine-Global Per-Session Markers Under HOME

A FLOW subcommand that resolves a session id and writes a
per-session marker file beneath the user's home directory is a
distinct leak surface from the dotfile reads in "The Rule" above:
the spawned child does not merely READ ambient state, it WRITES
machine-global state that a concurrently-running real flow depends
on. The canonical case is `bin/flow phase-enter`, which resolves a
session id (from `CLAUDE_CODE_SESSION_ID`, then the SessionStart
capture file) and writes the phase-anchor marker at
`<HOME>/.claude/flow/phase-anchor-<session-id>.json`
(`src/phase_anchor.rs`). `bin/flow set-utility-in-progress` writes
an analogous per-session marker under the same directory.

When a test spawns such a subcommand WITHOUT neutralizing both
`HOME` and `CLAUDE_CODE_SESSION_ID`, and the test suite happens to
run inside an active flow, the child resolves the REAL session id
under the REAL home and overwrites that flow's live marker with a
tempdir path — corrupting the very `--continue-step` resume the
marker exists to support.

Two neutralizers close this surface, and BOTH are required:

1. `HOME=<fixture>` — points the child's home at the disposable
   per-test tempdir, so any marker write lands under the fixture
   (cleaned up with it) rather than the engineer's real home.
2. `env_remove("CLAUDE_CODE_SESSION_ID")` — so the child cannot
   resolve the live session id from the inherited env; the id
   resolves only from the fixture's SessionStart capture file
   (normally absent, so no marker is written at all).

`HOME` alone is insufficient as a safety property argument but is
sufficient in practice to redirect the WRITE away from the real
home: even with the live session id resolved from the inherited
env, the marker lands under the fixture home. Removing
`CLAUDE_CODE_SESSION_ID` additionally makes the resolved id
deterministic for tests that assert on the marker's basename. A
test that asserts the marker lands under the fixture sets both; a
test that only needs to avoid clobbering the real marker needs at
minimum `HOME=<fixture>`.

## Working Directory Isolation

Environment neutralization (above) controls what the child reads
from the environment. The child's **working directory** is a
second leak surface: a spawned FLOW binary resolves the active
branch — and therefore the `.flow-states/<branch>/state.json` it
reads — from its cwd. A subprocess test that does NOT set
`.current_dir(...)` inherits the test runner's cwd (the real
worktree), so the child resolves the REAL flow's branch and reads
its live state file. When an in-flight field such as
`_halt_pending` is set on that real state, a hook under test reads
it and changes its decision (a halt gate blocks an
otherwise-allowed call, for example), producing a failure that
depends on the surrounding flow's state rather than on the test's
fixture.

This is the spawned-binary sibling of
`.claude/rules/testing-gotchas.md` "Host Environment Leaks": that
rule covers an in-process call to `current_branch()` /
`project_root()`; this covers a spawned binary that runs the same
resolution from its inherited cwd.

### The Rule

Every test that spawns a FLOW binary whose code path reads the
state file — hook validators especially (`validate-skill`,
`validate-ask-user`, `validate-pretool`, `validate-worktree-paths`,
`stop_*`) — MUST set `.current_dir(fixture_root)` to a directory
that does NOT resolve to an active flow: either a plain tempdir
(no `.flow-states/` entry for the resolved branch) or a fixture
worktree with a state file the test controls. Inheriting the
runner's cwd is forbidden — it couples the test's outcome to
whatever flow happens to be active when CI runs.

### How to Apply

When writing or reviewing a subprocess test that spawns a FLOW
binary, confirm `.current_dir(...)` points at a fixture. The
symptom of the missing call is a test that passes in isolation
and on a fresh flow but fails mid-flow when an in-flight state
field is set; the fix is to add `.current_dir(fixture_root)`.

## Canonical Helper Pattern

Subprocess tests that are NOT hook invocations should define a
no-recursion helper at the top of the test file and go through it
exclusively:

```rust
fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}
```

Tests that drive external-service code paths add the required
env neutralizers at the call site:

```rust
let tmp = tempfile::tempdir().expect("tempdir");
let root = tmp.path().canonicalize().expect("canonicalize");
let output = flow_rs_no_recursion()
    .args(["issue", "--title", "x", "--body-file", "/nonexistent"])
    .current_dir(&root)
    .env("GIT_CEILING_DIRECTORIES", &root)
    .env("GH_TOKEN", "invalid")  // gh auth fails fast, no network
    .env("HOME", &root)           // no ~/.config/gh, no ~/.gitconfig
    .output()
    .expect("spawn flow-rs issue");
```

### Hook Subprocess Tests Route Through the Shared Helper

Tests that spawn the `bin/flow hook <name>` subcommand go through
the shared `crate::common::spawn_hook(hook_name, cwd, stdin, env)`
helper in `tests/common/mod.rs` rather than a per-file
`flow_rs_no_recursion`. The shared helper centralizes the recursion
guard plus the worktree-isolation surface this rule mandates: it
removes `FLOW_CI_RUNNING`, `FLOW_SIMULATE_BRANCH`, and `HOME`, sets
`.current_dir(cwd)`, pipes all three stdio streams, then applies the
caller-supplied `env` pairs last (last-write-wins, so a test that
needs a specific `HOME` or `FLOW_SIMULATE_BRANCH` passes it
explicitly via the `env` slice). `stdin` is `&[u8]` so a non-UTF-8
robustness payload stays expressible, and the helper returns the
child's `Output`.

The per-file `flow_rs_no_recursion` pattern above remains correct
for subprocess tests that are NOT hook invocations: non-hook
subcommands, BrokenPipe-tolerant stdin probes, and spawns that need
a `pre_exec` hook (`setsid`, `rmdir`) the shared helper does not
expose. For those, define the per-file helper and pass the
neutralizers at the call site.

## When to Apply Which Neutralizers

Map the subcommand the test invokes to the services its module
reaches, and neutralize exactly those:

| Subcommand family | Services reached | Required neutralizers |
|---|---|---|
| `bin/flow issue`, `close-issue`, `close-issues`, `link-blocked-by`, `auto-close-parent`, `label-issues` | `gh` CLI → GitHub API | `GH_TOKEN=invalid`, `HOME=<tempdir>` |
| `bin/flow notify-slack` | Slack webhook POST | `env_remove("SLACK_WEBHOOK_URL")`, `env_remove("SLACK_BOT_TOKEN")` |
| `bin/flow ci`, `build`, `test`, `lint`, `format` | recursion guard | `env_remove("FLOW_CI_RUNNING")` |
| `bin/flow hook <name>` | state file reads, stdin | routed through `crate::common::spawn_hook`, which removes `HOME` and sets `.current_dir(fixture)` so the hook resolves a fixture branch/state, not the real worktree (see "Working Directory Isolation"); pass a specific `HOME` via the `env` slice when the hook must read one |
| `bin/flow phase-enter`, `set-utility-in-progress` (any subcommand that writes a per-session marker under `<HOME>/.claude/flow/`) | per-session marker write under HOME; session id resolved from `CLAUDE_CODE_SESSION_ID` then the SessionStart capture file | `HOME=<fixture>` (redirects the marker write off the real home) AND `env_remove("CLAUDE_CODE_SESSION_ID")` (the child resolves only the fixture session id, never the live one) — see "Machine-Global Per-Session Markers Under HOME". Both are required; without them a suite run inside an active flow overwrites that flow's live marker and corrupts `--continue-step` resume |

## Plan-Phase Trigger

When a plan task adds a test that spawns a subprocess:

1. Identify which services the subject subcommand reaches
   (check the module's `run` function for `Command::new("gh")`,
   `reqwest::*`, cloud SDK calls, etc.)
2. List the env neutralizers in the Risks section of the plan
3. Add a test helper or per-test env setup that applies every
   listed neutralizer

A subprocess test that omits a required neutralizer is a
Plan-phase gap.

## How to Apply (Review)

When the reviewer, pre-mortem, or adversarial agents find a
subprocess test lacking env neutralization:

1. File the gap as a Real finding mapped to Tenant 5 (Test
   coverage)
2. Fix by adding the missing `env_remove` / `env` calls at the
   spawn site OR extending a shared helper
3. Verify by running the test in an environment with the
   credential set (e.g., `GH_TOKEN=<real-token> bin/flow ci --test
   -- <test>`) — the test must still pass without making
   network calls
