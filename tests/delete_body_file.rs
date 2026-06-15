//! Validation-matrix tests for the `delete-body-file` subcommand.
//!
//! Drives the public library surface `run_impl(&Args, cwd)` (the outcome
//! core) and `run_impl_main(&Args, cwd)` (the JSON envelope). One test per
//! branch; the 100% coverage gate is the named consumer. Each test trips
//! when its covered line is removed.

use std::fs;
use std::path::Path;

use flow_rs::delete_body_file::{run_impl, run_impl_main, Args};

fn args(path: &str) -> Args {
    Args {
        path: path.to_string(),
    }
}

// --- validation rejections (Err) ---

#[test]
fn empty_path_rejected() {
    let cwd = Path::new("/tmp");
    let err = run_impl(&args(""), cwd).expect_err("empty --path must reject");
    assert!(
        err.contains("empty"),
        "empty-path error must name 'empty'; got: {}",
        err
    );
}

#[test]
fn relative_dotdot_traversal_rejected() {
    let cwd = Path::new("/tmp");
    let err = run_impl(&args("../.flow-issue-body-x"), cwd)
        .expect_err("`..` traversal in a relative path must reject");
    assert!(
        err.contains("forbidden") && err.contains("traversal"),
        "dotdot rejection error must name 'forbidden' and 'traversal'; got: {}",
        err
    );
}

#[test]
fn absolute_dotdot_traversal_rejected_and_preserves_victim() {
    // Regression: an absolute path containing `..` previously skipped the
    // traversal guard (it was relative-only) and `fs::remove_file` resolved
    // the `..` through the OS, deleting a file outside the target directory.
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let victim = dir.path().join(".flow-issue-body-victim");
    fs::write(&victim, "do not delete me").unwrap();

    let escaping = format!("{}/../.flow-issue-body-victim", sub.to_str().unwrap());
    let err = run_impl(&args(&escaping), Path::new("/nonexistent"))
        .expect_err("an absolute `..` path must reject");

    assert!(
        err.contains("forbidden") && err.contains("traversal"),
        "absolute dotdot rejection must name 'forbidden' and 'traversal'; got: {}",
        err
    );
    assert!(
        victim.exists(),
        "the absolute `..` path must not delete a file outside the target directory"
    );
}

#[test]
fn non_body_basename_rejected_and_preserves_file() {
    // Regression: an absolute path to an arbitrary regular file (not a
    // `.flow-issue-body` temp file) must be refused before removal.
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("important.txt");
    fs::write(&victim, "unrelated file").unwrap();

    let err = run_impl(&args(victim.to_str().unwrap()), Path::new("/nonexistent"))
        .expect_err("a non-issue-body basename must reject");
    assert!(
        err.contains("is not a") && err.contains(".flow-issue-body"),
        "basename rejection must name the '.flow-issue-body' family; got: {}",
        err
    );
    assert!(victim.exists(), "an unrelated file must survive rejection");
}

#[test]
fn symlinked_parent_with_foreign_basename_rejected() {
    // Regression (adversarial): a regular file reached through a symlinked
    // parent directory must not be deleted. The basename allowlist rejects
    // the foreign basename before the symlinked parent can be followed.
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("realdir");
    fs::create_dir(&real_dir).unwrap();
    let real_file = real_dir.join("real.txt");
    fs::write(&real_file, "behind a symlinked parent").unwrap();
    let link_dir = dir.path().join("linkdir");
    symlink(&real_dir, &link_dir).unwrap();

    let through_link = link_dir.join("real.txt");
    let err = run_impl(
        &args(through_link.to_str().unwrap()),
        Path::new("/nonexistent"),
    )
    .expect_err("a foreign basename behind a symlinked parent must reject");
    assert!(
        err.contains(".flow-issue-body"),
        "rejection must name the '.flow-issue-body' family; got: {}",
        err
    );
    assert!(
        real_file.exists(),
        "the file behind the symlinked parent must survive"
    );
}

#[test]
fn root_path_with_no_basename_rejected() {
    // `/` has no final component; the basename allowlist's None arm rejects it.
    let err = run_impl(&args("/"), Path::new("/nonexistent"))
        .expect_err("a path with no final component must reject");
    assert!(
        err.contains(".flow-issue-body"),
        "rejection must name the '.flow-issue-body' family; got: {}",
        err
    );
}

#[test]
fn symlink_target_rejected_and_preserved() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    fs::write(&target, "preserve me").unwrap();
    // The link itself carries an allowlisted basename so it reaches the
    // regular-file check (rather than being rejected by basename).
    let link = dir.path().join(".flow-issue-body-link");
    symlink(&target, &link).unwrap();

    let err = run_impl(&args(link.to_str().unwrap()), dir.path())
        .expect_err("a symlink target must reject");
    assert!(
        err.contains("not a regular file"),
        "symlink rejection must name 'not a regular file'; got: {}",
        err
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "preserve me",
        "the symlink target must survive — delete must not follow the link"
    );
}

#[test]
fn directory_target_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join(".flow-issue-body-dir");
    fs::create_dir(&sub).unwrap();

    let err = run_impl(&args(sub.to_str().unwrap()), dir.path())
        .expect_err("a directory target must reject");
    assert!(
        err.contains("not a regular file"),
        "directory rejection must name 'not a regular file'; got: {}",
        err
    );
    assert!(sub.exists(), "the directory must survive rejection");
}

// --- successful outcomes (Ok) ---

#[test]
fn absolute_existing_file_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body-abc");
    fs::write(&file, "body").unwrap();

    // An absolute path is accepted directly (it carries an allowlisted
    // basename and no `..`). The cwd argument is irrelevant here.
    let outcome = run_impl(&args(file.to_str().unwrap()), Path::new("/nonexistent")).unwrap();
    assert_eq!(outcome, "deleted");
    assert!(!file.exists(), "the file must be removed");
}

#[test]
fn relative_existing_file_deleted_resolves_against_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body-rel");
    fs::write(&file, "body").unwrap();

    // A relative path without `..` resolves against the injected cwd.
    let outcome = run_impl(&args(".flow-issue-body-rel"), dir.path()).unwrap();
    assert_eq!(outcome, "deleted");
    assert!(!file.exists(), "the file must be removed");
}

#[test]
fn nonexistent_target_returns_missing() {
    let dir = tempfile::tempdir().unwrap();
    let outcome = run_impl(&args(".flow-issue-body-missing"), dir.path()).unwrap();
    assert_eq!(
        outcome, "missing",
        "a NotFound target is tolerated as 'missing', not an error"
    );
}

#[test]
fn unremovable_target_returns_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    let file = locked.join(".flow-issue-body-locked");
    fs::write(&file, "cannot remove me").unwrap();
    // Mode 000 on the parent dir blocks traversal and removal: both the
    // stat and the `fs::remove_file` fail with a non-NotFound error,
    // yielding the `error` outcome rather than a panic.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let outcome = run_impl(&args(file.to_str().unwrap()), dir.path()).unwrap();

    // Restore permissions so the TempDir can be cleaned up.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        outcome, "error",
        "an un-removable target reports 'error', never panics"
    );
}

// --- JSON envelope (run_impl_main) ---

#[test]
fn run_impl_main_ok_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body-env");
    fs::write(&file, "body").unwrap();

    let (value, code) = run_impl_main(&args(file.to_str().unwrap()), dir.path());
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["outcome"], "deleted");
    assert!(!file.exists());
}

#[test]
fn run_impl_main_err_envelope() {
    let (value, code) = run_impl_main(&args(""), Path::new("/tmp"));
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(
        value["message"].as_str().unwrap().contains("empty"),
        "error envelope message must name 'empty'; got: {}",
        value["message"]
    );
}
