//! Adversarial probe for the per-session utility marker / Stop-hook PR.
//!
//! Probes target:
//! 1. `src/commands/utility_marker.rs::write_marker` — `fs::write` runs without a
//!    symlink-safe pre-check, which the project rule
//!    `.claude/rules/rust-patterns.md` "Symlink-Safe Existence Checks Before Writes"
//!    explicitly forbids for any `fs::write` into a user-controlled directory.
//! 2. `src/commands/utility_marker.rs::marker_path` — does not require `home`
//!    to be absolute. Per `.claude/rules/external-input-path-construction.md`
//!    "Validate env-var-derived paths as absolute," an empty / non-absolute
//!    `home` produces a cwd-relative path the caller did not authorize.
//! 3. `src/hooks/stop_continue.rs::check_in_progress_utility_skill` — when HOME
//!    is empty (the value `home_dir_or_empty()` returns when `$HOME` is unset),
//!    `marker_path` produces a cwd-relative path. A hostile or accidental
//!    `.claude/flow/utility-in-progress-<id>.json` in the process cwd then
//!    triggers a spurious turn-end refusal — defeating the predicate's
//!    "no marker → no block" contract.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use flow_rs::commands::utility_marker::{marker_path, write_marker};
use flow_rs::hooks::stop_continue::check_in_progress_utility_skill;

const TEST_SKILL: &str = "flow:flow-create-issue";
const TEST_SESSION: &str = "abc12345";

// --- Probe 1: symlink escape via fs::write at marker path ---

/// Pre-creates a symlink at the canonical marker path that points outside
/// the home dir. Production `write_marker` uses `fs::write(&path, ...)`
/// without a symlink-aware pre-check, so the write follows the symlink
/// and overwrites the link target. The rule
/// `.claude/rules/rust-patterns.md` "Symlink-Safe Existence Checks Before
/// Writes" requires `fs::symlink_metadata(&path).is_ok()` (or equivalent
/// `is_symlink()` rejection) before any `fs::write` into a user-owned
/// directory tree. The marker file lives under `<HOME>/.claude/flow/` —
/// exactly the user-owned-directory shape the rule names.
#[test]
fn write_marker_does_not_follow_symlink_to_external_target() {
    let home_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let home = home_dir.path().canonicalize().unwrap();
    let target = target_dir.path().canonicalize().unwrap();

    // Pre-create the marker parent and a symlink at the marker path that
    // points outside the home dir.
    let marker_parent = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_parent).unwrap();
    let marker = marker_parent.join(format!("utility-in-progress-{}.json", TEST_SESSION));
    let escape_target = target.join("attacker_overwrote_me");
    // Pre-write the escape target with content the attacker hopes is preserved.
    fs::write(&escape_target, b"sentinel-content").unwrap();
    symlink(&escape_target, &marker).unwrap();

    let _ = write_marker(&home, TEST_SKILL, TEST_SESSION);

    // The escape target must still hold the sentinel — write_marker
    // must NOT have followed the symlink. Either it should reject
    // (Err) or replace the symlink with a regular file in-place.
    let escape_after = fs::read(&escape_target).unwrap_or_default();
    assert_eq!(
        escape_after, b"sentinel-content",
        "write_marker followed a symlink and overwrote a file outside HOME — \
         see .claude/rules/rust-patterns.md \"Symlink-Safe Existence Checks Before Writes\""
    );
}

// --- Probe 2: marker_path returns Some for empty home ---

/// `marker_path(home, session_id)` returns `Some` whenever the
/// session_id passes `is_safe_session_id`, regardless of whether
/// `home` is a valid absolute path. An empty / non-absolute `home`
/// produces a cwd-relative path that resolves wherever the caller
/// happens to be. Per `.claude/rules/external-input-path-construction.md`
/// "Validate env-var-derived paths as absolute," the function (or its
/// callers) must reject empty / non-absolute `home` before joining —
/// a non-absolute `home` is a category of invalid input distinct from
/// an invalid `session_id`, and `marker_path` is the natural choke
/// point. The reference implementation `read_rate_limits(home: &Path)`
/// in `src/window_snapshot.rs` early-returns on empty/non-absolute
/// `home` for exactly this reason.
#[test]
fn marker_path_rejects_empty_home() {
    let result = marker_path(&PathBuf::new(), TEST_SESSION);
    assert!(
        result.is_none(),
        "marker_path must reject empty home so a cwd-relative path \
         cannot escape the caller's intended directory"
    );
}

// --- Probe 3: check_in_progress_utility_skill blocks under empty HOME on cwd-relative path ---

/// `home_dir_or_empty()` (the helper threaded into the Stop hook for
/// `home`) returns an empty `PathBuf` when `$HOME` is unset.
/// `marker_path(&empty_path, session_id)` then produces the cwd-
/// relative path `.claude/flow/utility-in-progress-<session>.json`.
/// If the process's cwd contains such a file (a hostile repo could
/// check one in, or a stray `.claude/flow/` directory could exist
/// for any other reason), `check_in_progress_utility_skill` opens
/// it and refuses turn-end — a spurious block that defeats the
/// predicate's "no marker → no block" contract.
///
/// Per `.claude/rules/external-input-path-construction.md`
/// "Validate env-var-derived paths as absolute," the predicate (or
/// its helper) must reject empty / non-absolute `home` before
/// constructing the marker path so a cwd-relative resolve is
/// impossible.
#[test]
fn check_in_progress_utility_skill_does_not_treat_cwd_as_home_when_home_empty() {
    // Build a fake "cwd" that contains a marker file at the relative path
    // `.claude/flow/utility-in-progress-<id>.json` — what an attacker would
    // plant in a malicious repo, or a stray dotfile from another tool.
    let cwd_dir = tempfile::tempdir().unwrap();
    let cwd = cwd_dir.path().canonicalize().unwrap();
    let stash = cwd.join(".claude").join("flow");
    fs::create_dir_all(&stash).unwrap();
    let payload = format!(
        r#"{{"skill":"{}","session_id":"{}","started_at":"2026-05-09T12:00:00-07:00"}}"#,
        TEST_SKILL, TEST_SESSION
    );
    fs::write(
        stash.join(format!("utility-in-progress-{}.json", TEST_SESSION)),
        payload,
    )
    .unwrap();

    // Run the predicate from `cwd_dir` with an empty HOME path — the
    // shape `home_dir_or_empty()` produces when `$HOME` is unset.
    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&cwd).unwrap();

    let result = check_in_progress_utility_skill(TEST_SESSION, &PathBuf::new());

    // Restore cwd before any assertion can short-circuit.
    let _ = std::env::set_current_dir(&original_cwd);

    assert!(
        !result.should_block,
        "check_in_progress_utility_skill must not interpret cwd as HOME — \
         empty home + cwd-relative marker_path produces a spurious turn-end refusal. \
         See .claude/rules/external-input-path-construction.md \
         \"Validate env-var-derived paths as absolute\""
    );
}
