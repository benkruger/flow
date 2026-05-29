//! Integration tests for `src/session_cost.rs` — per-session cost
//! reads and monthly aggregation. Every fixture path is
//! canonicalized at construction so prefix comparisons hold across
//! macOS `/var` ↔ `/private/var` symlinks per
//! `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
//! Canonicalization".

use std::fs;

use tempfile::TempDir;

use flow_rs::session_cost::{cost_file_path, read_monthly_aggregate};

// --- cost_file_path ---

/// `cost_file_path` resolves to
/// `<project_root>/.claude/cost/<YYYY-MM>/<session_id>` with no
/// extension — matching the producer in
/// `~/.claude/statusline-command.sh`.
#[test]
fn cost_file_path_resolves_no_extension_under_year_month_dir() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let path = cost_file_path(&root, "sid-abc").expect("valid sid");
    let expected = root
        .join(".claude")
        .join("cost")
        .join(&year_month)
        .join("sid-abc");
    assert_eq!(path, expected);
    assert!(
        path.file_name().unwrap().to_string_lossy() == "sid-abc",
        "no extension allowed"
    );
}

/// `cost_file_path` rejects a traversal-containing session_id at
/// its own boundary — the pub function validates regardless of
/// upstream caller discipline.
#[test]
fn cost_file_path_rejects_traversal_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    assert_eq!(cost_file_path(&root, "../escape"), None);
    assert_eq!(cost_file_path(&root, ".."), None);
    assert_eq!(cost_file_path(&root, "."), None);
}

/// `cost_file_path` rejects an empty session_id.
#[test]
fn cost_file_path_rejects_empty_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    assert_eq!(cost_file_path(&root, ""), None);
}

/// `cost_file_path` rejects a session_id containing a path
/// separator — the join would otherwise create a subdirectory
/// the discovery walkers cannot see.
#[test]
fn cost_file_path_rejects_session_id_with_slash() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    assert_eq!(cost_file_path(&root, "sid/with/slash"), None);
}

// --- read_monthly_aggregate ---

/// Task 4: `read_monthly_aggregate` sums every session cost file
/// under the current month's directory.
#[test]
fn session_cost_read_monthly_aggregate_sums_session_files() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir");
    fs::write(cost_dir.join("sid-1"), "1.50").expect("write 1");
    fs::write(cost_dir.join("sid-2"), "2.25").expect("write 2");
    fs::write(cost_dir.join("sid-3"), "0.10").expect("write 3");

    let total = read_monthly_aggregate(&root);
    assert!((total - 3.85).abs() < 1e-9, "expected 3.85, got {}", total);
}

/// Missing month directory → aggregate is 0.0 (no panic, no error).
#[test]
fn read_monthly_aggregate_returns_zero_when_directory_absent() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let total = read_monthly_aggregate(&root);
    assert_eq!(total, 0.0);
}

/// Corrupted entries are skipped silently; valid entries still
/// contribute. A single bad file cannot suppress the aggregate.
#[test]
fn read_monthly_aggregate_skips_corrupt_entries() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir");
    fs::write(cost_dir.join("sid-good"), "5.00").expect("write good");
    fs::write(cost_dir.join("sid-bad"), "garbage").expect("write bad");
    fs::write(cost_dir.join("sid-infinity"), "inf").expect("write inf");

    let total = read_monthly_aggregate(&root);
    assert!((total - 5.00).abs() < 1e-9, "expected 5.00, got {}", total);
}

/// Subdirectory inside the cost dir (e.g. a stray `.tmp/` folder)
/// is read attempted as a file; `fs::read_to_string` fails and the
/// entry is skipped without aborting the loop.
#[test]
fn read_monthly_aggregate_skips_directory_entries() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir");
    fs::create_dir_all(cost_dir.join("subdir")).expect("mkdir subdir");
    fs::write(cost_dir.join("sid-1"), "1.00").expect("write");

    let total = read_monthly_aggregate(&root);
    assert!((total - 1.00).abs() < 1e-9);
}

// --- byte cap ---

/// Plant an oversized cost file under the cost dir and assert
/// `read_monthly_aggregate` completes without OOM. The per-entry
/// take cap bounds the walker's worst-case I/O.
#[test]
fn read_monthly_aggregate_with_oversized_entry_completes_bounded() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir");
    let mut content = String::from("3.25");
    content.push_str(&" ".repeat(2 * 1024 * 1024));
    fs::write(cost_dir.join("sid-oversized"), &content).expect("write");
    let total = read_monthly_aggregate(&root);
    assert!(
        (total - 3.25).abs() < 1e-9,
        "expected 3.25 from truncated read, got {}",
        total
    );
}

/// Plant a dangling symlink as a cost-dir entry on Unix —
/// `fs::File::open` returns Err (ENOENT) and the walker's inner
/// `if let Ok(file)` branch is skipped (the dangling symlink is
/// silently dropped without aborting the loop, real files still
/// contribute).
#[cfg(unix)]
#[test]
fn read_monthly_aggregate_skips_dangling_symlink_entries() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir");
    fs::write(cost_dir.join("sid-real"), "7.50").expect("write real");
    // Dangling symlink — points at a nonexistent target so
    // fs::File::open fails (ENOENT).
    symlink(
        cost_dir.join("nonexistent-target"),
        cost_dir.join("sid-dangling"),
    )
    .expect("symlink");

    let total = read_monthly_aggregate(&root);
    assert!(
        (total - 7.50).abs() < 1e-9,
        "expected 7.50 (dangling symlink skipped), got {}",
        total
    );
}
