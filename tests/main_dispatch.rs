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
