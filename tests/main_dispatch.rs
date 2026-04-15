//! Dispatch tests for every `flow-rs` subcommand.
//!
//! Spawns `flow-rs <subcommand> --help` for each subcommand name. Clap
//! auto-generates help text and exits 0, so a passing test confirms:
//!   1. The subcommand is reachable from the `Commands` enum dispatch.
//!   2. Clap can parse the subcommand's arguments (help path).
//!   3. The subcommand's name matches what the enum declares.
//!
//! Mechanical coverage — not a semantic assertion about what the command
//! does. Each entry adds ~2-4 regions in `src/main.rs` dispatch code.

use std::process::Command;

fn help_exits_ok(subcommand: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg(subcommand)
        .arg("--help")
        .output()
        .expect("failed to spawn flow-rs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "subcommand '{}' --help exited {:?}\nstderr: {}",
        subcommand,
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    // --help output contains "Usage:" (clap's header).
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage:"),
        "subcommand '{}' --help output missing 'Usage:' header\ngot: {}",
        subcommand,
        stdout
    );
}

#[test]
fn all_subcommands_have_working_help() {
    // Enumerated from the Commands enum in src/main.rs. Each name must
    // match the #[command(name = "...")] attribute (hyphenated) or the
    // enum variant name lowercased (for variants without explicit name).
    let subcommands = [
        "bump-version",
        "check-freshness",
        "check-phase",
        "phase-transition",
        "ci",
        "build",
        "test",
        "lint",
        "format",
        "update-deps",
        "analyze-issues",
        "append-note",
        "add-finding",
        "add-issue",
        "add-notification",
        "cleanup",
        "issue",
        "close-issue",
        "close-issues",
        "create-sub-issue",
        "link-blocked-by",
        "create-milestone",
        "extract-release-notes",
        "prime-check",
        "prime-setup",
        "promote-permissions",
        "auto-close-parent",
        "complete-fast",
        "complete-preflight",
        "complete-merge",
        "complete-finalize",
        "complete-post-merge",
        "set-timestamp",
        "set-blocked",
        "clear-blocked",
        "init-state",
        "log",
        "generate-id",
        "start-lock",
        "start-step",
        "start-finalize",
        "start-gate",
        "start-init",
        "start-workspace",
        "format-status",
        "session-context",
        "label-issues",
        "format-issues-summary",
        "format-complete-summary",
        "format-pr-timings",
        "finalize-commit",
        "notify-slack",
        "write-rule",
        "phase-enter",
        "phase-finalize",
        "plan-check",
        "plan-extract",
        "render-pr-body",
        "update-pr-body",
        "orchestrate-report",
        "orchestrate-state",
        "tombstone-audit",
        "tui",
        "tui-data",
        "upgrade-check",
        "qa-mode",
        "qa-reset",
        "qa-verify",
        "scaffold-qa",
        "hook",
    ];
    for sub in subcommands {
        help_exits_ok(sub);
    }
}

#[test]
fn top_level_help_exits_ok() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("--help")
        .output()
        .expect("failed to spawn flow-rs");
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
}

#[test]
fn hook_subcommands_help_exits_ok() {
    // Hook is itself a subcommand with nested subcommands.
    let hooks = [
        "validate-pretool",
        "validate-claude-paths",
        "validate-worktree-paths",
        "validate-ask-user",
        "stop-continue",
        "stop-failure",
        "post-compact",
    ];
    for hook_name in hooks {
        let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
            .args(["hook", hook_name, "--help"])
            .output()
            .expect("failed to spawn");
        assert_eq!(
            output.status.code(),
            Some(0),
            "hook '{}' --help exited {:?}\nstderr: {}",
            hook_name,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn invalid_subcommand_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("not-a-real-command")
        .output()
        .expect("failed to spawn");
    // Clap exits 2 (or passes through to external handler); confirm non-zero.
    assert_ne!(
        output.status.code(),
        Some(0),
        "invalid subcommand should exit non-zero"
    );
}

// --- Dispatch arms covered end-to-end via subprocess ---
//
// These tests exercise the match arms in `main.rs` that call
// `dispatch::dispatch_json` / `dispatch::dispatch_text` /
// `process::exit`. In-process unit tests of each module's
// `run_impl_main` validate the return tuple; these subprocess tests
// confirm that main.rs wires each `run_impl_main` result to the right
// stdout/stderr/exit-code triple.

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    // Prevent recursion-guard triggers when `bin/flow ci` spawns these
    // subprocesses during the wider test suite run. Not strictly needed
    // for these subcommands (they aren't CI-tier runners), but defensive
    // per .claude/rules/rust-patterns.md "Guard Universality".
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `flow-rs` invoked with no subcommand writes an error to stderr and
/// exits 1 — covers the `None` arm in `fn main`.
#[test]
fn no_command_writes_stderr_and_exits_1() {
    let output = flow_rs_no_recursion().output().expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("flow-rs: no command specified"),
        "stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("--help"),
        "stderr should mention --help: {}",
        stderr
    );
}

/// A bare unknown token exits 127 via the `External(_)` arm — tighter
/// than the sibling `invalid_subcommand_errors` test which only
/// asserts non-zero.
#[test]
fn external_arm_exits_127() {
    let output = flow_rs_no_recursion()
        .arg("this-subcommand-does-not-exist-and-never-will")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(127));
}

/// `bin/flow check-phase --required flow-start` takes the first-phase
/// short-circuit in `check_phase::run_impl_main` and exits 0 silently.
/// Exercises the `dispatch_text` path end-to-end.
#[test]
fn check_phase_first_phase_exits_0() {
    let output = flow_rs_no_recursion()
        .args(["check-phase", "--required", "flow-start", "--branch", "any"])
        .output()
        .expect("spawn flow-rs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on first-phase short-circuit, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// `bin/flow tui-data` with no flag writes a stderr error and exits 1
/// — covers the `Err` branch of `tui_data::run_impl_main`.
#[test]
fn tui_data_no_flag_writes_stderr_and_exits_1() {
    let output = flow_rs_no_recursion()
        .arg("tui-data")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui-data: specify one of"),
        "stderr: {}",
        stderr
    );
}

/// `bin/flow tui-data --load-all-flows` exits 0 with a JSON array on
/// stdout — covers the `Ok(Value, 0)` + `dispatch_json` path.
#[test]
fn tui_data_load_all_flows_exits_0_with_array() {
    let output = flow_rs_no_recursion()
        .args(["tui-data", "--load-all-flows"])
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim_start().starts_with('['),
        "stdout should be a JSON array: {}",
        stdout
    );
}

/// `bin/flow start-lock` round-trip covers the three functional branches
/// of `start_lock::run()` (`--acquire`, `--check`, `--release`)
/// end-to-end via the CLI dispatch path.
///
/// Unit tests in `src/commands/start_lock.rs` cover the `acquire`,
/// `acquire_with_wait`, `release`, and `check` library functions in
/// isolation. The two concurrency tests in `tests/concurrency.rs`
/// (`thundering_herd_zero_delay`, `start_lock_serialization`) call the
/// library functions directly to avoid fork/exec contention under
/// nextest. Without this round-trip, the `start_lock::run()` dispatch
/// layer in `src/commands/start_lock.rs` — the code that parses CLI
/// flags, resolves `project_root()`, and wires the library return
/// values to stdout JSON — would have zero integration coverage.
///
/// The test uses an isolated tempdir for the queue directory and sets
/// `GIT_CEILING_DIRECTORIES` so `project_root()`'s `git worktree list`
/// call cannot walk up to a parent git repo and pollute a real
/// `.flow-states/start-queue/`. With no reachable git repo, the
/// subprocess falls back to `PathBuf::from(".")` which canonicalizes
/// to the tempdir cwd.
#[test]
fn start_lock_cli_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // 1) --acquire on an empty queue exits 0 with status=acquired.
    //    Exercises the `--acquire` branch and the `queue_path` →
    //    `acquire()` call chain inside `start_lock::run()`.
    let output = flow_rs_no_recursion()
        .args(["start-lock", "--acquire", "--feature", "cli-roundtrip"])
        .current_dir(tmp.path())
        .env("GIT_CEILING_DIRECTORIES", tmp.path())
        .output()
        .expect("spawn flow-rs start-lock --acquire");
    assert_eq!(
        output.status.code(),
        Some(0),
        "start-lock --acquire stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let acquire_stdout = String::from_utf8_lossy(&output.stdout);
    let acquire_json: serde_json::Value = serde_json::from_str(acquire_stdout.trim())
        .expect("start-lock --acquire stdout must be JSON");
    assert_eq!(
        acquire_json["status"], "acquired",
        "acquire output: {}",
        acquire_json
    );

    // 2) --check on a held lock exits 0 with status=locked and the
    //    feature name of the holder. Exercises the `--check` branch.
    let output = flow_rs_no_recursion()
        .args(["start-lock", "--check"])
        .current_dir(tmp.path())
        .env("GIT_CEILING_DIRECTORIES", tmp.path())
        .output()
        .expect("spawn flow-rs start-lock --check");
    assert_eq!(output.status.code(), Some(0));
    let check_stdout = String::from_utf8_lossy(&output.stdout);
    let check_json: serde_json::Value =
        serde_json::from_str(check_stdout.trim()).expect("start-lock --check stdout must be JSON");
    assert_eq!(check_json["status"], "locked");
    assert_eq!(check_json["feature"], "cli-roundtrip");

    // 3) --release exits 0 with status=released. Exercises the
    //    `--release` branch and proves the queue entry was unlinked.
    let output = flow_rs_no_recursion()
        .args(["start-lock", "--release", "--feature", "cli-roundtrip"])
        .current_dir(tmp.path())
        .env("GIT_CEILING_DIRECTORIES", tmp.path())
        .output()
        .expect("spawn flow-rs start-lock --release");
    assert_eq!(
        output.status.code(),
        Some(0),
        "start-lock --release stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let release_stdout = String::from_utf8_lossy(&output.stdout);
    let release_json: serde_json::Value = serde_json::from_str(release_stdout.trim())
        .expect("start-lock --release stdout must be JSON");
    assert_eq!(release_json["status"], "released");

    // 4) --check on a released lock exits 0 with status=free,
    //    confirming the release actually unlinked the queue entry
    //    rather than reporting success in error.
    let output = flow_rs_no_recursion()
        .args(["start-lock", "--check"])
        .current_dir(tmp.path())
        .env("GIT_CEILING_DIRECTORIES", tmp.path())
        .output()
        .expect("spawn flow-rs start-lock --check");
    assert_eq!(output.status.code(), Some(0));
    let check_stdout = String::from_utf8_lossy(&output.stdout);
    let check_json: serde_json::Value =
        serde_json::from_str(check_stdout.trim()).expect("start-lock --check stdout must be JSON");
    assert_eq!(check_json["status"], "free");
}
