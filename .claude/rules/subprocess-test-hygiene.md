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
  profraw files to its cwd. When the child's cwd is the worktree
  root, those files persist into `git status`.
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
     uses: `AWS_ACCESS_KEY_ID`, `GOOGLE_APPLICATION_CREDENTIALS`,
     `AZURE_STORAGE_ACCOUNT` — `env_remove`
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

## Canonical Helper Pattern

Every test file that spawns the binary should define a
no-recursion helper at the top and go through it exclusively:

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

## When to Apply Which Neutralizers

Map the subcommand the test invokes to the services its module
reaches, and neutralize exactly those:

| Subcommand family | Services reached | Required neutralizers |
|---|---|---|
| `bin/flow issue`, `close-issue`, `create-sub-issue`, `link-blocked-by`, `create-milestone`, `auto-close-parent`, `label-issues` | `gh` CLI → GitHub API | `GH_TOKEN=invalid`, `HOME=<tempdir>` |
| `bin/flow notify-slack` | Slack webhook POST | `env_remove("SLACK_WEBHOOK_URL")`, `env_remove("SLACK_BOT_TOKEN")` |
| `bin/flow ci`, `build`, `test`, `lint`, `format` | recursion guard | `env_remove("FLOW_CI_RUNNING")` |
| `bin/flow hook <name>` | state file reads, stdin | `HOME=<tempdir>` if the hook might read ~/.config |

## Plan-Phase Trigger

When a plan task adds a test that spawns a subprocess:

1. Identify which services the subject subcommand reaches
   (check the module's `run` function for `Command::new("gh")`,
   `reqwest::*`, cloud SDK calls, etc.)
2. List the env neutralizers in the Risks section of the plan
3. Add a test helper or per-test env setup that applies every
   listed neutralizer

A subprocess test that omits a required neutralizer is a
Plan-phase gap — the adversarial and pre-mortem agents will
catch it in Code Review, but the cheaper catch is at Plan time.

## How to Apply (Code Review)

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

## Enforcement

Iteration 1 is rule-prose only. A future contract test in
`tests/subprocess_hygiene.rs` could scan every `Command::new`
in `tests/**/*.rs` for missing env-neutralization patterns; see
the filed plugin issue for the enforcement proposal.

## Cross-References

- `.claude/rules/testing-gotchas.md` — Rust parallel test env
  var races; the "never `set_var` in tests" discipline is the
  sibling rule for inside-process env safety, this rule covers
  the child-process side.
- `.claude/rules/concurrency-model.md` — shared GitHub state
  coordination; the rule explains why neutralizing `GH_TOKEN`
  matters (real API calls mutate shared state across engineers).
