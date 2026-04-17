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

/// `flow-rs finalize-commit "" ""` passes Clap but fails the
/// empty-args check in `run_impl`, exercising the `run()` →
/// `json_error` → `process::exit(1)` path.
#[test]
fn finalize_commit_empty_args_exits_1() {
    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "", ""])
        .output()
        .expect("spawn flow-rs finalize-commit");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error") || stdout.contains("Usage"),
        "expected error in stdout, got: {}",
        stdout
    );
}

/// `flow-rs init-state ""` exercises the empty-name guard in the
/// `InitState` arm body of `main.rs` — the `if feature_name.is_empty()`
/// branch fires `json_error` + `process::exit(1)` before reaching the
/// `commands::init_state::run` delegation.
#[test]
fn main_init_state_empty_name_exits_1() {
    let output = flow_rs_no_recursion()
        .args(["init-state", ""])
        .output()
        .expect("spawn flow-rs init-state");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Feature name required"),
        "expected stdout to contain 'Feature name required', got: {}",
        stdout
    );
    assert!(
        stdout.contains("\"step\":\"args\""),
        "expected stdout JSON to contain `\"step\":\"args\"`, got: {}",
        stdout
    );
}

/// `flow-rs init-state "valid-name"` in an isolated tempdir exits 1
/// because no `.flow.json` exists. Covers the delegation line of the
/// `InitState` arm body — the `commands::init_state::run` call that
/// runs after the empty-name guard passes.
#[test]
fn main_init_state_valid_name_no_flow_json_exits_1() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize tempdir");
    let output = flow_rs_no_recursion()
        .args(["init-state", "valid-name"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs init-state");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Could not read .flow.json"),
        "expected stdout to surface the .flow.json read failure, got: {}",
        stdout
    );
}

/// `flow-rs start-step --step 1 --branch <valid> echo ok` covers the
/// `StartStep` arm's delegation to `commands::start_step::run`. The
/// arm body is now a thin pass-through after the dead `--`-strip
/// branch was removed (clap's `trailing_var_arg` consumes `--` as a
/// separator and never leaves it in the trailing-args vec).
#[test]
fn main_start_step_no_double_dash_passes_through() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize tempdir");
    let output = flow_rs_no_recursion()
        .args([
            "start-step",
            "--step",
            "1",
            "--branch",
            "test-fixture-branch",
            "echo",
            "ok",
        ])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs start-step");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `flow-rs format-status` in a tempdir with no git repo exits 2
/// because `resolve_branch(None, &root)` returns `None` (no branch
/// override, no git repo to detect from). Covers the `Err` arm of
/// the `FormatStatus` arm body — the `eprintln!` + `process::exit`
/// path that fires when `format_status::run_impl_main` returns
/// `Err(("Could not determine current branch", 2))`.
#[test]
fn main_format_status_branch_resolution_err_exits_nonzero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize tempdir");
    let output = flow_rs_no_recursion()
        .arg("format-status")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs format-status");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 from branch-resolution failure\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Could not determine current branch"),
        "expected stderr to surface the branch-resolution failure message, got: {}",
        stderr
    );
    assert!(
        !stderr.contains("panicked at"),
        "format-status must not panic, got: {}",
        stderr
    );
}

/// `flow-rs tui` invoked via subprocess (no controlling TTY on the
/// child) exits 1. Covers the `Tui` arm body in `main.rs` and the
/// production `tui_terminal::run_tui_arm` wrapper, which detects the
/// non-TTY case and returns `Err(("Error: ...", 1))`.
#[test]
fn main_tui_non_tty_exits_1() {
    let output = flow_rs_no_recursion()
        .arg("tui")
        .output()
        .expect("spawn flow-rs tui");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("flow tui requires an interactive terminal"),
        "expected stderr to surface the non-TTY rejection message, got: {}",
        stderr
    );
}

/// Parameterized sweep that invokes every thin `Some(Commands::X(args)) =>
/// X::run(args)` arm in `src/main.rs` to drive the arm body's region
/// instrumentation. Each entry passes the minimal args clap requires;
/// each invocation runs in an isolated tempdir with `GIT_CEILING_DIRECTORIES`
/// set so the spawned child cannot escape into the host repo. Exit codes
/// vary (most subcommands exit 1 on missing state, some exit 0 cleanly,
/// hooks exit 0 on empty stdin). The assertion is uniform: stderr must
/// not contain `"panicked at"`. The test's job is to enter the arm
/// body — what the underlying `X::run` does once entered is the owning
/// module's coverage concern, out of scope for `main.rs` 100%.
///
/// Arms `Cleanup`, `FinalizeCommit`, `StartLock`, `CheckPhase`, `TuiData`,
/// `InitState`, `StartStep`, `Tui`, `External`, and `None` are covered
/// by dedicated tests above and are intentionally excluded from the
/// sweep.
#[test]
fn main_arm_invocations_cover_dispatch() {
    use std::io::Write;
    use std::process::Stdio;

    fn run_sweep_entry(
        subcommand: &str,
        args: &[&str],
        stdin_json: Option<&str>,
    ) -> std::process::Output {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonicalize tempdir");
        let mut cmd = flow_rs_no_recursion();
        cmd.arg(subcommand);
        cmd.args(args);
        cmd.current_dir(&root);
        cmd.env("GIT_CEILING_DIRECTORIES", &root);
        // Neutralize gh CLI auth so any subcommand that shells out to
        // `gh` (close-issue, create-sub-issue, link-blocked-by,
        // create-milestone, auto-close-parent, label-issues, qa-reset,
        // qa-verify, scaffold-qa) fails with an immediate auth error
        // rather than blocking on a network timeout in CI environments
        // without GitHub credentials. `GH_TOKEN=invalid` forces gh to
        // fail at the auth check before issuing any HTTP request.
        // Setting `HOME` to the tempdir prevents gh from reading a
        // user-level config that could supply a real token.
        cmd.env("GH_TOKEN", "invalid");
        cmd.env("HOME", &root);
        if let Some(input) = stdin_json {
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            let mut child = cmd.spawn().expect("spawn flow-rs");
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(input.as_bytes())
                .expect("write stdin");
            child.wait_with_output().expect("wait_with_output")
        } else {
            cmd.output().expect("spawn flow-rs")
        }
    }

    fn run_hook_sweep(hook_name: &str) -> std::process::Output {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonicalize tempdir");
        let mut cmd = flow_rs_no_recursion();
        cmd.args(["hook", hook_name]);
        cmd.current_dir(&root);
        cmd.env("GIT_CEILING_DIRECTORIES", &root);
        // Same neutralization as run_sweep_entry — hook subcommands
        // can also shell out to `gh` (e.g., stop-failure capturing
        // PR context).
        cmd.env("GH_TOKEN", "invalid");
        cmd.env("HOME", &root);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn flow-rs hook");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(b"{}")
            .expect("write stdin");
        child.wait_with_output().expect("wait_with_output")
    }

    // (subcommand, args, optional stdin) — each entry hits one thin arm.
    let invocations: &[(&str, &[&str], Option<&str>)] = &[
        ("bump-version", &[], None),
        ("check-freshness", &[], None),
        ("ci", &[], None),
        ("build", &[], None),
        ("test", &[], None),
        ("lint", &[], None),
        ("format", &[], None),
        ("update-deps", &[], None),
        ("analyze-issues", &[], None),
        (
            "append-note",
            &["--note", "x", "--branch", "test-fixture"],
            None,
        ),
        (
            "add-finding",
            &[
                "--finding",
                "x",
                "--reason",
                "y",
                "--outcome",
                "fixed",
                "--phase",
                "flow-code",
                "--branch",
                "test-fixture",
            ],
            None,
        ),
        (
            "add-issue",
            &[
                "--label",
                "Tech Debt",
                "--title",
                "x",
                "--url",
                "u",
                "--phase",
                "flow-code",
                "--branch",
                "test-fixture",
            ],
            None,
        ),
        (
            "add-notification",
            &[
                "--phase",
                "flow-code",
                "--ts",
                "1.0",
                "--message",
                "m",
                "--branch",
                "test-fixture",
            ],
            None,
        ),
        (
            "issue",
            &["--title", "x", "--body-file", "/nonexistent"],
            None,
        ),
        (
            "close-issue",
            &["--repo", "x/y", "--issue-number", "1"],
            None,
        ),
        ("close-issues", &[], None),
        (
            "create-sub-issue",
            &["--repo", "x/y", "--parent", "1", "--child", "2"],
            None,
        ),
        (
            "link-blocked-by",
            &[
                "--repo",
                "x/y",
                "--blocked-number",
                "1",
                "--blocking-number",
                "2",
            ],
            None,
        ),
        ("create-milestone", &["--repo", "x/y", "--title", "m"], None),
        ("extract-release-notes", &["1.0.0"], None),
        ("prime-check", &[], None),
        ("prime-setup", &[], None),
        ("promote-permissions", &[], None),
        (
            "auto-close-parent",
            &["--repo", "x/y", "--parent", "1"],
            None,
        ),
        ("complete-fast", &["--branch", "test-fixture"], None),
        ("complete-preflight", &["--branch", "test-fixture"], None),
        (
            "complete-merge",
            &["--pr", "1", "--state-file", "/nonexistent/state.json"],
            None,
        ),
        (
            "complete-finalize",
            &[
                "--pr",
                "1",
                "--state-file",
                "/nonexistent/state.json",
                "--branch",
                "test-fixture",
                "--worktree",
                ".worktrees/test-fixture",
            ],
            None,
        ),
        (
            "complete-post-merge",
            &[
                "--pr",
                "1",
                "--state-file",
                "/nonexistent/state.json",
                "--branch",
                "test-fixture",
            ],
            None,
        ),
        (
            "set-timestamp",
            &["--set", "x=1", "--branch", "test-fixture"],
            None,
        ),
        ("set-blocked", &[], None),
        ("clear-blocked", &[], None),
        ("log", &["test-fixture", "msg"], None),
        ("generate-id", &[], None),
        (
            "start-finalize",
            &["--branch", "test-fixture", "--pr-url", "u"],
            None,
        ),
        ("start-gate", &["--branch", "test-fixture"], None),
        ("start-init", &["test-fixture"], None),
        (
            "start-workspace",
            &["test-fixture", "--branch", "test-fixture"],
            None,
        ),
        ("session-context", &[], None),
        ("label-issues", &["--repo", "x/y"], None),
        ("format-issues-summary", &["--branch", "test-fixture"], None),
        (
            "format-complete-summary",
            &["--state-file", "/nonexistent/state.json"],
            None,
        ),
        (
            "format-pr-timings",
            &[
                "--state-file",
                "/nonexistent/state.json",
                "--output",
                "/dev/null",
            ],
            None,
        ),
        ("format-status", &["--branch", "test-fixture"], None),
        (
            "notify-slack",
            &["--phase", "flow-code", "--message", "m"],
            None,
        ),
        (
            "write-rule",
            &["--path", "/nonexistent", "--content", "x"],
            None,
        ),
        (
            "phase-enter",
            &["--phase", "flow-code", "--branch", "test-fixture"],
            None,
        ),
        (
            "phase-finalize",
            &["--phase", "flow-code", "--branch", "test-fixture"],
            None,
        ),
        ("plan-check", &[], None),
        ("plan-extract", &[], None),
        (
            "render-pr-body",
            &["--pr", "1", "--branch", "test-fixture"],
            None,
        ),
        ("update-pr-body", &["--branch", "test-fixture"], None),
        ("orchestrate-report", &[], None),
        ("orchestrate-state", &["--init"], None),
        ("tombstone-audit", &[], None),
        ("upgrade-check", &[], None),
        (
            "qa-mode",
            &["--start", "--local-path", "/nonexistent"],
            None,
        ),
        ("qa-reset", &["--repo", "x/y"], None),
        ("qa-verify", &["--repo", "x/y"], None),
        (
            "scaffold-qa",
            &["--template", "nonexistent", "--repo", "x/y"],
            None,
        ),
    ];

    for (sub, args, stdin) in invocations {
        let output = run_sweep_entry(sub, args, *stdin);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked at"),
            "subcommand `{}` panicked unexpectedly:\nstderr: {}",
            sub,
            stderr
        );
        assert!(
            !stderr.contains("thread 'main' panicked"),
            "subcommand `{}` panicked on main thread:\nstderr: {}",
            sub,
            stderr
        );
    }

    // Hook subcommands receive empty JSON `{}` on stdin so they early-
    // return on missing required fields rather than blocking.
    let hooks = [
        "validate-pretool",
        "validate-claude-paths",
        "validate-worktree-paths",
        "validate-ask-user",
        "stop-continue",
        "stop-failure",
        "post-compact",
    ];
    for hook in hooks {
        let output = run_hook_sweep(hook);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked at"),
            "hook `{}` panicked unexpectedly:\nstderr: {}",
            hook,
            stderr
        );
        assert!(
            !stderr.contains("thread 'main' panicked"),
            "hook `{}` panicked on main thread:\nstderr: {}",
            hook,
            stderr
        );
    }
}

/// `flow-rs upgrade-check` with `FLOW_PLUGIN_JSON` pointing at a
/// valid plugin.json exercises `upgrade_check::run()` end-to-end —
/// read plugin.json, parse version/repository, spawn `run_gh_cmd`
/// against the real `gh` CLI (with `GH_TOKEN=invalid` so the auth
/// check fails fast), print the JSON result, exit 0. Covers the
/// two untested functions (`run`, `run_gh_cmd`) via subprocess
/// instrumentation per `.claude/rules/no-waivers.md`.
#[test]
fn upgrade_check_run_with_plugin_json_exits_0() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let plugin_json = root.join("plugin.json");
    std::fs::write(
        &plugin_json,
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar-does-not-exist-fixture"}"#,
    )
    .expect("write plugin.json");

    let output = flow_rs_no_recursion()
        .arg("upgrade-check")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("FLOW_UPGRADE_TIMEOUT", "5")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs upgrade-check");

    assert_eq!(
        output.status.code(),
        Some(0),
        "upgrade-check always exits 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("upgrade-check stdout must be JSON");
    let status = json["status"].as_str().unwrap_or("");
    assert!(
        status == "unknown" || status == "current" || status == "upgrade_available",
        "status should be unknown/current/upgrade_available, got: {}\nfull JSON: {}",
        status,
        json
    );
}

/// `flow-rs upgrade-check` with `FLOW_PLUGIN_JSON` pointing at a
/// nonexistent file exercises the read-error branch in `run()` →
/// `upgrade_check_impl`. The status is `unknown` and the reason
/// cites the read failure; `run_gh_cmd` is NOT called because the
/// read error short-circuits before the gh dispatch.
#[test]
fn upgrade_check_run_missing_plugin_json_exits_0_unknown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let missing = root.join("absent.json");

    let output = flow_rs_no_recursion()
        .arg("upgrade-check")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("FLOW_PLUGIN_JSON", &missing)
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs upgrade-check");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("upgrade-check stdout must be JSON");
    assert_eq!(json["status"], "unknown");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("Could not read plugin.json"),
        "reason should name read failure, got: {}",
        reason
    );
}

/// `flow-rs upgrade-check` with `FLOW_PLUGIN_JSON` pointing at a
/// plugin.json that contains invalid JSON exercises the parse-error
/// branch in `upgrade_check_impl` via the CLI entry point.
#[test]
fn upgrade_check_run_invalid_plugin_json_exits_0_unknown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let plugin_json = root.join("bad.json");
    std::fs::write(&plugin_json, "not valid json {{{").expect("write plugin.json");

    let output = flow_rs_no_recursion()
        .arg("upgrade-check")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs upgrade-check");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("upgrade-check stdout must be JSON");
    assert_eq!(json["status"], "unknown");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("Invalid plugin.json"),
        "reason should name parse failure, got: {}",
        reason
    );
}

/// `flow-rs upgrade-check` with `FLOW_PLUGIN_JSON` pointing at a
/// plugin.json that is missing the `version` field exercises the
/// no-version branch in `upgrade_check_impl`.
#[test]
fn upgrade_check_run_no_version_field_exits_0_unknown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let plugin_json = root.join("no-version.json");
    std::fs::write(
        &plugin_json,
        r#"{"repository":"https://github.com/foo/bar"}"#,
    )
    .expect("write plugin.json");

    let output = flow_rs_no_recursion()
        .arg("upgrade-check")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs upgrade-check");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("upgrade-check stdout must be JSON");
    assert_eq!(json["status"], "unknown");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("No version in plugin.json"),
        "reason should name missing-version, got: {}",
        reason
    );
}

/// Reachability test: `flow-rs upgrade-check` with `FLOW_UPGRADE_TIMEOUT=0`
/// reaches `run_gh_cmd` via the `run()` entry point. The outcome is not
/// a deterministic timeout — on a machine without `gh` on PATH the call
/// short-circuits to `GhResult::NotFound`, on a machine with `gh` the
/// deadline (now + 0s) fires. Either outcome proves the `run_gh_cmd`
/// dispatch was reached. A dedicated timeout-branch test would need to
/// pin `gh` on PATH, which CI cannot guarantee.
#[test]
fn upgrade_check_run_reaches_run_gh_cmd_dispatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let plugin_json = root.join("plugin.json");
    std::fs::write(
        &plugin_json,
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    )
    .expect("write plugin.json");

    let output = flow_rs_no_recursion()
        .arg("upgrade-check")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("FLOW_UPGRADE_TIMEOUT", "0")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs upgrade-check");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("upgrade-check stdout must be JSON");
    assert_eq!(json["status"], "unknown");
    let reason = json["reason"].as_str().unwrap_or("");
    // Discriminate between the two acceptable terminal outcomes so a
    // regression that produces neither (empty reason, different JSON
    // shape, panic) is surfaced — not just "some reason was set."
    assert!(
        reason.contains("timed out") || reason.contains("not found") || reason.contains("failed"),
        "reason must name a run_gh_cmd outcome (timed out / not found / failed), got: {}",
        reason
    );
}

/// `flow-rs cleanup /nonexistent --branch test --worktree .worktrees/test`
/// exercises the `run()` → `json_error` → `process::exit(1)` path
/// for an invalid project root.
#[test]
fn cleanup_invalid_root_exits_1() {
    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            "/nonexistent/path/that/does/not/exist",
            "--branch",
            "test",
            "--worktree",
            ".worktrees/test",
        ])
        .output()
        .expect("spawn flow-rs cleanup");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("not found") || stdout.contains("error"),
        "expected error message in stdout, got: {}",
        stdout
    );
}
