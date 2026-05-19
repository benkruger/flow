//! Tests for `src/shared_config_approval.rs` — the branch-scoped,
//! per-file, single-use shared-config approval marker store.
//!
//! The marker store is the "proceed" half of the shared-config gate:
//! `bin/flow approve-shared-config` writes a marker after the user
//! approves an `AskUserQuestion`, and
//! `validate_worktree_paths::validate_shared_config` consults and
//! consumes it immediately before its block return. The contract this
//! file locks in: single-use consumption (a marker authorizes exactly
//! one edit), per-file scope (an approval for path A never satisfies a
//! check for path B), corruption resilience (any unreadable/unparseable
//! marker fails closed → no approval → the gate still blocks), and
//! branch-path-safety (a `/`/`.`/`..`/NUL/empty branch never reaches
//! filesystem path construction and never panics).

use std::fs;

use flow_rs::shared_config_approval::{
    check_and_consume_approval, clear_all, marker_path, write_approval,
};

// --- marker_path ---

#[test]
fn marker_path_valid_branch_is_some_under_branch_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").expect("valid branch yields a path");
    let s = p.to_string_lossy();
    assert!(
        s.contains(".flow-states/feat-x/shared-config-approvals/"),
        "marker must live under the branch-scoped approvals dir: {s}"
    );
}

#[test]
fn marker_path_invalid_branch_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(marker_path(root, "", "/repo/Cargo.toml").is_none());
    assert!(marker_path(root, ".", "/repo/Cargo.toml").is_none());
    assert!(marker_path(root, "..", "/repo/Cargo.toml").is_none());
    assert!(marker_path(root, "a/b", "/repo/Cargo.toml").is_none());
    assert!(marker_path(root, "a\0b", "/repo/Cargo.toml").is_none());
}

#[test]
fn marker_path_distinct_targets_distinct_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    let b = marker_path(root, "feat-x", "/repo/.gitignore").unwrap();
    assert_ne!(a, b, "distinct target paths must map to distinct markers");
}

// --- write_approval / check_and_consume_approval ---

#[test]
fn write_then_consume_returns_true_once_then_false() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").expect("write succeeds");
    assert!(
        check_and_consume_approval(root, "feat-x", "/repo/Cargo.toml"),
        "first consume returns true"
    );
    assert!(
        !check_and_consume_approval(root, "feat-x", "/repo/Cargo.toml"),
        "second consume returns false (single-use)"
    );
}

#[test]
fn consume_deletes_the_marker_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").expect("write succeeds");
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    assert!(p.exists(), "marker exists after write");
    assert!(check_and_consume_approval(root, "feat-x", "/repo/Cargo.toml"));
    assert!(!p.exists(), "marker deleted after consume");
}

#[test]
fn consume_without_marker_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn per_file_scope_approval_for_a_does_not_satisfy_b() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").expect("write succeeds");
    assert!(
        !check_and_consume_approval(root, "feat-x", "/repo/.gitignore"),
        "approval for Cargo.toml must not satisfy a check for .gitignore"
    );
    // The Cargo.toml approval is untouched and still consumable.
    assert!(check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn per_branch_scope_approval_under_a_does_not_satisfy_b() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").expect("write succeeds");
    assert!(
        !check_and_consume_approval(root, "feat-y", "/repo/Cargo.toml"),
        "approval written under feat-x must not satisfy feat-y"
    );
}

// --- corruption resilience (fail closed → no approval) ---

#[test]
fn corruption_empty_marker_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "").unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn corruption_non_json_marker_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "not json {{{").unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn corruption_wrong_root_type_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "[1, 2, 3]").unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn corruption_approved_false_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"approved": false, "target": "/repo/Cargo.toml"}"#).unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn corruption_target_mismatch_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Hand-write a marker at Cargo.toml's path slot but with a
    // mismatched `target` field — per-file scope is enforced at the
    // marker too (defense-in-depth alongside the filename key).
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"approved": true, "target": "/repo/.gitignore"}"#).unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

// --- branch-path-safety (never panic, never approve) ---

#[test]
fn invalid_branch_check_returns_false_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for b in ["", ".", "..", "a/b", "a\0b"] {
        assert!(
            !check_and_consume_approval(root, b, "/repo/Cargo.toml"),
            "invalid branch {b:?} must yield no approval"
        );
    }
}

#[test]
fn invalid_branch_write_returns_err_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for b in ["", ".", "..", "a/b", "a\0b"] {
        assert!(
            write_approval(root, b, "/repo/Cargo.toml").is_err(),
            "invalid branch {b:?} must fail to write"
        );
    }
}

#[test]
fn write_approval_errors_when_root_is_a_file() {
    // root is a regular file, so `<root>/.flow-states/<branch>/...`
    // cannot be created — `fs::create_dir_all` returns Err and
    // `write_approval` surfaces it rather than silently approving.
    let dir = tempfile::tempdir().unwrap();
    let file_root = dir.path().join("not-a-dir");
    fs::write(&file_root, "x").unwrap();
    assert!(write_approval(&file_root, "feat-x", "/repo/Cargo.toml").is_err());
}

#[test]
fn corruption_non_utf8_marker_no_approval() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = marker_path(root, "feat-x", "/repo/Cargo.toml").unwrap();
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    // Invalid UTF-8 bytes — read_to_string fails, no approval.
    fs::write(&p, [0xff_u8, 0xfe, 0xfd]).unwrap();
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
}

#[test]
fn invalid_branch_clear_all_is_noop_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for b in ["", ".", "..", "a/b", "a\0b"] {
        clear_all(root, b);
    }
}

// --- clear_all ---

#[test]
fn clear_all_removes_every_marker_for_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").unwrap();
    write_approval(root, "feat-x", "/repo/.gitignore").unwrap();
    clear_all(root, "feat-x");
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/Cargo.toml"
    ));
    assert!(!check_and_consume_approval(
        root,
        "feat-x",
        "/repo/.gitignore"
    ));
}

#[test]
fn clear_all_missing_dir_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // No markers ever written — clear_all must not error or panic.
    clear_all(root, "feat-x");
}

#[test]
fn clear_all_does_not_touch_other_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_approval(root, "feat-x", "/repo/Cargo.toml").unwrap();
    write_approval(root, "feat-y", "/repo/Cargo.toml").unwrap();
    clear_all(root, "feat-x");
    assert!(
        check_and_consume_approval(root, "feat-y", "/repo/Cargo.toml"),
        "clear_all(feat-x) must leave feat-y's marker intact"
    );
}
