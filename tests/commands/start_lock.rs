//! Tests for `src/commands/start_lock.rs`.
//!
//! Exercises queue management (`list_queue`, `queue_path`), lock
//! state transitions (`acquire`, `acquire_with_wait`, `release`,
//! `check`), and the CLI dispatcher (`run_impl_main`). Tests drive
//! through the public surface only — `acquire_with_wait`'s retry
//! loop uses `thread::sleep` with real short intervals when a test
//! needs to cross the retry boundary.

use std::fs;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use filetime::{set_file_mtime, FileTime};
use flow_rs::commands::start_lock::{
    acquire, acquire_with_wait, check, list_queue, queue_path, release, run_impl_main,
    QUEUE_DIRNAME,
};
use flow_rs::flow_paths::FlowStatesDir;

// --- queue_path tests ---

#[test]
fn test_queue_path_creates_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let qp = queue_path(&root);
    assert!(qp.is_dir());
    assert_eq!(qp, FlowStatesDir::new(&root).path().join(QUEUE_DIRNAME));
}

#[test]
fn test_queue_path_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let qp1 = queue_path(root);
    let qp2 = queue_path(root);
    assert_eq!(qp1, qp2);
    assert!(qp2.is_dir());
}

// --- list_queue tests ---

#[test]
fn test_list_queue_empty() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    let (entries, stale) = list_queue(queue_dir, false);
    assert!(entries.is_empty());
    assert!(!stale);
}

#[test]
fn test_list_queue_sorted_by_mtime_then_name() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let older = queue_dir.join("beta");
    fs::write(&older, "").unwrap();
    set_file_mtime(
        &older,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(20)),
    )
    .unwrap();

    let newer = queue_dir.join("alpha");
    fs::write(&newer, "").unwrap();
    set_file_mtime(
        &newer,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let (entries, _) = list_queue(queue_dir, false);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].1, "beta"); // older first
    assert_eq!(entries[1].1, "alpha"); // newer second
}

#[test]
fn test_list_queue_tiebreaker_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let same_time = SystemTime::now() - Duration::from_secs(10);
    for name in ["charlie", "alpha"] {
        let path = queue_dir.join(name);
        fs::write(&path, "").unwrap();
        set_file_mtime(&path, FileTime::from_system_time(same_time)).unwrap();
    }

    let (entries, _) = list_queue(queue_dir, false);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].1, "alpha");
    assert_eq!(entries[1].1, "charlie");
}

#[test]
fn test_list_queue_skips_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::create_dir(queue_dir.join("subdir")).unwrap();
    fs::write(queue_dir.join("real-entry"), "").unwrap();

    let (entries, _) = list_queue(queue_dir, false);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, "real-entry");
}

#[test]
fn test_list_queue_cleanup_removes_stale() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale = queue_dir.join("old-feature");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
    )
    .unwrap();

    let fresh = queue_dir.join("new-feature");
    fs::write(&fresh, "").unwrap();

    let (entries, stale_removed) = list_queue(queue_dir, true);
    assert!(stale_removed);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, "new-feature");
    assert!(!stale.exists());
}

#[test]
fn test_list_queue_no_cleanup_preserves_stale_file() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale = queue_dir.join("old-feature");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
    )
    .unwrap();

    let (entries, stale_flag) = list_queue(queue_dir, false);
    assert!(stale_flag);
    assert!(entries.is_empty());
    assert!(stale.exists());
}

#[test]
fn test_list_queue_nonexistent_dir() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().join("nonexistent");
    let (entries, stale) = list_queue(&queue_dir, false);
    assert!(entries.is_empty());
    assert!(!stale);
}

#[test]
fn test_list_queue_future_mtime_not_stale() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let future_entry = queue_dir.join("future-feature");
    fs::write(&future_entry, "").unwrap();
    set_file_mtime(
        &future_entry,
        FileTime::from_system_time(SystemTime::now() + Duration::from_secs(3600)),
    )
    .unwrap();

    let (entries, stale_found) = list_queue(queue_dir, true);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, "future-feature");
    assert!(!stale_found);
    assert!(future_entry.exists());
}

// --- acquire tests ---

#[test]
fn test_acquire_empty_queue() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let result = acquire("test-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
    assert!(queue_dir.join("test-feature").exists());
    assert_eq!(result["lock_path"], queue_dir.display().to_string());
}

#[test]
fn test_acquire_locked_by_older_entry() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let older = queue_dir.join("alpha-feature");
    fs::write(&older, "").unwrap();
    set_file_mtime(
        &older,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let result = acquire("beta-feature", queue_dir);
    assert_eq!(result["status"], "locked");
    assert_eq!(result["feature"], "alpha-feature");
    assert!(queue_dir.join("beta-feature").exists());
    assert_eq!(result["lock_path"], queue_dir.display().to_string());
}

#[test]
fn test_acquire_stale_cleanup_acquires() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale = queue_dir.join("old-feature");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
    )
    .unwrap();

    let result = acquire("new-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
    assert_eq!(result["stale_broken"], true);
    assert!(!stale.exists());
    assert!(queue_dir.join("new-feature").exists());
}

#[test]
fn test_acquire_tiebreaker_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let same_time = SystemTime::now() - Duration::from_secs(10);
    for name in ["charlie-feature", "alpha-feature"] {
        let path = queue_dir.join(name);
        fs::write(&path, "").unwrap();
        set_file_mtime(&path, FileTime::from_system_time(same_time)).unwrap();
    }

    let result = acquire("delta-feature", queue_dir);
    assert_eq!(result["status"], "locked");
    assert_eq!(result["feature"], "alpha-feature");
}

#[test]
fn test_acquire_idempotent_when_first() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::write(queue_dir.join("my-feature"), "").unwrap();

    let result = acquire("my-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
}

#[test]
fn test_acquire_replaces_own_stale_entry() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale_self = queue_dir.join("my-feature");
    fs::write(&stale_self, "").unwrap();
    // 2-hour-old mtime (7200s) to put the entry well past
    // STALE_TIMEOUT_SECONDS regardless of filesystem mtime precision
    // or the delta between set_file_mtime and the acquire() call's
    // SystemTime::now().
    set_file_mtime(
        &stale_self,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(7200)),
    )
    .unwrap();
    let initial_mtime = fs::metadata(&stale_self)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap();

    let result = acquire("my-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
    assert!(queue_dir.join("my-feature").exists());
    let final_mtime = fs::metadata(queue_dir.join("my-feature"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap();
    assert!(
        final_mtime > initial_mtime,
        "entry mtime must advance after stale-replace; initial={:?} final={:?}",
        initial_mtime,
        final_mtime
    );
}

#[test]
fn test_acquire_skips_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::create_dir(queue_dir.join("subdir")).unwrap();

    let result = acquire("my-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
}

#[test]
fn test_acquire_stale_cleanup_preserves_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale = queue_dir.join("aaa-stale");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
    )
    .unwrap();

    let fresh = queue_dir.join("bbb-fresh");
    fs::write(&fresh, "").unwrap();
    set_file_mtime(
        &fresh,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let result = acquire("ccc-new", queue_dir);
    assert_eq!(result["status"], "locked");
    assert_eq!(result["feature"], "bbb-fresh");
    assert_eq!(result["stale_broken"], true);
    assert!(!stale.exists());
    assert!(fresh.exists());
    assert!(queue_dir.join("ccc-new").exists());
}

// --- acquire_with_wait_impl tests (seam) ---

#[test]
fn test_acquire_with_wait_immediate() {
    // Empty queue → acquire_with_wait succeeds without entering the
    // retry loop (no thread::sleep, no retry branch).
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let result = acquire_with_wait("test-feature", queue_dir, 90, 10);
    assert_eq!(result["status"], "acquired");
}

#[test]
fn test_acquire_with_wait_succeeds_after_retry() {
    // Lock is held by `blocking-feature`; a background thread deletes
    // the blocking entry after a short delay. The foreground
    // acquire_with_wait polls every 1s (the minimum non-zero interval)
    // and eventually acquires the lock when the blocker goes away.
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().to_path_buf();

    let blocking = queue_dir.join("blocking-feature");
    fs::write(&blocking, "").unwrap();
    set_file_mtime(
        &blocking,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let blocking_clone = blocking.clone();
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        let _ = fs::remove_file(&blocking_clone);
    });

    let result = acquire_with_wait("new-feature", &queue_dir, 5, 1);
    releaser.join().unwrap();
    assert_eq!(result["status"], "acquired");
}

#[test]
fn test_acquire_with_wait_timeout() {
    // timeout=0 triggers an immediate timeout after the first
    // (blocked) attempt without waiting on real time.
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let blocking = queue_dir.join("blocking-feature");
    fs::write(&blocking, "").unwrap();
    set_file_mtime(
        &blocking,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let result = acquire_with_wait("new-feature", queue_dir, 0, 10);
    assert_eq!(result["status"], "timeout");
    assert_eq!(result["feature"], "blocking-feature");
    assert!(result["waited_seconds"].is_number());
}

// --- acquire_with_wait (real-sleep wrapper) ---

/// The wrapper `acquire_with_wait` forwards to
/// `acquire_with_wait_impl` with a real `thread::sleep` closure.
/// Driving an immediate acquisition (empty queue) reaches the wrapper
/// without waiting on real time.
#[test]
fn test_acquire_with_wait_wrapper_immediate_acquires() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let result = acquire_with_wait("wrapper-feature", queue_dir, 0, 1);
    assert_eq!(result["status"], "acquired");
    assert!(queue_dir.join("wrapper-feature").exists());
}

// --- release tests ---

#[test]
fn test_release_deletes_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::write(queue_dir.join("my-feature"), "").unwrap();

    let result = release("my-feature", queue_dir);
    assert_eq!(result["status"], "released");
    assert_eq!(result["lock_path"], queue_dir.display().to_string());
    assert_eq!(result["was_present"], true);
    assert!(!queue_dir.join("my-feature").exists());
}

#[test]
fn test_release_only_deletes_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::write(queue_dir.join("my-feature"), "").unwrap();
    fs::write(queue_dir.join("other-feature"), "").unwrap();

    let result = release("my-feature", queue_dir);
    assert_eq!(result["status"], "released");
    assert!(!queue_dir.join("my-feature").exists());
    assert!(queue_dir.join("other-feature").exists());
}

#[test]
fn test_release_idempotent_when_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let result = release("nonexistent", queue_dir);
    assert_eq!(result["status"], "released");
    assert_eq!(result["was_present"], false);
}

#[test]
fn release_error_when_file_persists() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().join("queue");
    fs::create_dir(&queue_dir).unwrap();
    let entry = queue_dir.join("locked-feature");
    fs::write(&entry, "").unwrap();

    // Drop guard restores permissions even if an assertion panics,
    // preventing leaked read-only dirs in the temp tree.
    struct PermGuard(std::path::PathBuf);
    impl Drop for PermGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _guard = PermGuard(queue_dir.clone());

    let result = release("locked-feature", &queue_dir);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap_or("")
        .contains("persists after unlink"));
    assert_eq!(result["was_present"], true);
}

// --- check tests ---

#[test]
fn test_check_when_free() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let result = check(queue_dir);
    assert_eq!(result["status"], "free");
    assert_eq!(result["lock_path"], queue_dir.display().to_string());
}

#[test]
fn test_check_when_locked() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    fs::write(queue_dir.join("some-feature"), "").unwrap();

    let result = check(queue_dir);
    assert_eq!(result["status"], "locked");
    assert_eq!(result["feature"], "some-feature");
}

#[test]
fn test_check_stale_returns_free_without_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();

    let stale = queue_dir.join("old-feature");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
    )
    .unwrap();

    let result = check(queue_dir);
    assert_eq!(result["status"], "free");
    assert!(stale.exists());
}

// --- run_impl_main tests ---

#[test]
fn run_impl_main_acquire_without_feature_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(true, false, false, None, false, 0, 0, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("--feature required for --acquire"));
}

#[test]
fn run_impl_main_acquire_with_feature_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(
        true,
        false,
        false,
        Some("cli-feature".to_string()),
        false,
        0,
        0,
        &root,
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "acquired");
    assert_eq!(value["lock_path"], queue_path(&root).display().to_string());
}

#[test]
fn run_impl_main_acquire_with_wait_immediately_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(
        true,
        false,
        false,
        Some("wait-feature".to_string()),
        true,
        0,
        1,
        &root,
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "acquired");
}

#[test]
fn run_impl_main_release_without_feature_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(false, true, false, None, false, 0, 0, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("--feature required for --release"));
}

#[test]
fn run_impl_main_release_with_feature_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let queue_dir = queue_path(&root);
    fs::write(queue_dir.join("drop-feature"), "").unwrap();

    let (value, code) = run_impl_main(
        false,
        true,
        false,
        Some("drop-feature".to_string()),
        false,
        0,
        0,
        &root,
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "released");
    assert_eq!(value["was_present"], true);
}

#[test]
fn run_impl_main_check_when_free() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(false, false, true, None, false, 0, 0, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "free");
}

#[test]
fn run_impl_main_no_flag_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(false, false, false, None, false, 0, 0, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Specify --acquire, --release, or --check"));
}

// --- Edge paths (uncovered branches) ---

/// queue_path's `canonicalize()` Err fallback — when `root` doesn't
/// exist, canonicalize fails and the fallback `root.to_path_buf()`
/// is used. create_dir_all then materializes the missing ancestry.
#[test]
fn queue_path_canonicalize_failure_falls_back_to_root() {
    let dir = tempfile::tempdir().unwrap();
    let missing_root = dir.path().join("missing-parent");
    let qp = queue_path(&missing_root);
    assert!(qp.to_string_lossy().contains("missing-parent"));
    assert!(qp.is_dir());
}

/// Covers the `result["stale_broken"] = json!(true)` branch inside
/// the defensive empty-queue arm: queue_dir is read-only so
/// fs::File::create fails AND fs::remove_file of a stale entry
/// also fails. list_queue reports (empty, stale_found=true) and the
/// defensive branch stamps stale_broken onto the acquired result.
#[test]
fn acquire_defensive_branch_with_stale_removed_flag() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().join("queue");
    fs::create_dir(&queue_dir).unwrap();

    let stale = queue_dir.join("stale-feature");
    fs::write(&stale, "").unwrap();
    set_file_mtime(
        &stale,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(7200)),
    )
    .unwrap();

    struct PermGuard(std::path::PathBuf);
    impl Drop for PermGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    // 0o555 permits read/execute but not write, blocking
    // fs::File::create and fs::remove_file inside queue_dir while
    // still allowing readdir for list_queue.
    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _guard = PermGuard(queue_dir.clone());

    let result = acquire("new-feature", &queue_dir);
    assert_eq!(result["status"], "acquired");
    assert_eq!(result["stale_broken"], true);
}

/// acquire with a non-existent queue_dir → fs::File::create fails
/// silently, list_queue returns empty, and the defensive
/// "entries.is_empty" branch returns "acquired". Covers the
/// "Should not happen" branch that exists as a last-resort fallback.
#[test]
fn acquire_with_nonexistent_queue_dir_returns_defensive_acquired() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().join("does-not-exist");
    let result = acquire("my-feature", &queue_dir);
    assert_eq!(result["status"], "acquired");
    assert_eq!(result["lock_path"], queue_dir.display().to_string());
    // stale_broken is absent because list_queue returned
    // (empty, false) — no entries seen at all.
    assert!(result.get("stale_broken").is_none());
}

/// Pre-epoch mtime exercises the `duration_since(UNIX_EPOCH)` Err
/// path inside `mtime_secs`, which returns 0.0. Callers treat 0.0 as
/// maximally stale, so the entry is excluded from list_queue's
/// returned list AND `stale_found` is reported true.
#[test]
fn list_queue_treats_entry_with_pre_epoch_mtime_as_stale() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    let pre_epoch = queue_dir.join("pre-epoch-feature");
    fs::write(&pre_epoch, "").unwrap();
    // If the filesystem rejects negative unix time, skip — the
    // normal-mtime tests cover the Ok arm.
    if set_file_mtime(&pre_epoch, FileTime::from_unix_time(-1_000_000_000, 0)).is_err() {
        return;
    }

    let (entries, stale) = list_queue(queue_dir, false);
    assert!(
        entries.iter().all(|(_, n)| n != "pre-epoch-feature"),
        "pre-epoch entry must be excluded: {:?}",
        entries
    );
    assert!(stale, "pre-epoch entry must be flagged as stale");
}

/// acquire on an own entry with a pre-epoch mtime: mtime_secs
/// returns 0.0 → stale check fires → remove + recreate with fresh
/// mtime → subsequent list_queue sees a fresh entry. Result is
/// "acquired" (no stale_broken because list_queue saw no stale
/// entries on its second pass).
#[test]
fn acquire_own_entry_with_pre_epoch_mtime_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path();
    let entry = queue_dir.join("my-feature");
    fs::write(&entry, "").unwrap();
    if set_file_mtime(&entry, FileTime::from_unix_time(-1_000_000_000, 0)).is_err() {
        return;
    }

    let result = acquire("my-feature", queue_dir);
    assert_eq!(result["status"], "acquired");
    // Entry still exists (replaced by fresh create, not deleted).
    assert!(entry.exists());
    // Fresh mtime proves the refresh happened.
    let new_mtime = fs::metadata(&entry).unwrap().modified().unwrap();
    let now = SystemTime::now();
    let age = now.duration_since(new_mtime).unwrap_or_default();
    assert!(
        age < Duration::from_secs(60),
        "entry mtime must be fresh (<60s), age={:?}",
        age
    );
}

/// Covers the retry-loop `continue` fall-through in acquire_with_wait
/// (line that executes when the retry's acquire returns "locked"
/// again). The releaser thread holds the lock briefly, waits >1 polling
/// interval, then releases — so acquire_with_wait must iterate at least
/// twice before succeeding.
#[test]
fn acquire_with_wait_loops_at_least_twice_before_acquiring() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().to_path_buf();
    let blocking = queue_dir.join("blocking");
    fs::write(&blocking, "").unwrap();
    set_file_mtime(
        &blocking,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();

    let blocking_clone = blocking.clone();
    let releaser = thread::spawn(move || {
        // Sleep >1s (the polling interval) so the retry loop iterates
        // at least twice before the blocker is removed.
        thread::sleep(Duration::from_millis(1100));
        let _ = fs::remove_file(&blocking_clone);
    });

    let start = std::time::Instant::now();
    let result = acquire_with_wait("new-feature", &queue_dir, 10, 1);
    releaser.join().unwrap();
    assert_eq!(result["status"], "acquired");
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "expected at least 1s elapsed (retry loop iterated), got {:?}",
        start.elapsed()
    );
}

/// Covers the real `thread::sleep` closure body inside
/// `acquire_with_wait`. A helper thread removes the blocking entry
/// shortly after the wrapper enters its retry loop, so the wrapper
/// exits via "acquired" rather than blocking for the full timeout.
/// Uses `interval=0` so `thread::sleep(Duration::from_secs(0))`
/// returns essentially immediately — the loop polls quickly.
#[test]
fn acquire_with_wait_wrapper_enters_sleep_loop() {
    let dir = tempfile::tempdir().unwrap();
    let queue_dir = dir.path().to_path_buf();
    let blocking = queue_dir.join("blocking-wrapper");
    fs::write(&blocking, "").unwrap();
    set_file_mtime(
        &blocking,
        FileTime::from_system_time(SystemTime::now() - Duration::from_secs(5)),
    )
    .unwrap();

    let blocking_clone = blocking.clone();
    let helper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        let _ = fs::remove_file(&blocking_clone);
    });

    let result = acquire_with_wait("new-wrapper-feature", &queue_dir, 5, 0);
    helper.join().unwrap();
    assert_eq!(result["status"], "acquired");
}

// --- CLI subprocess tests ---
//
// Guard the `main.rs` `StartLock` arm wiring (clap parsing → run_impl_main
// → dispatch_json). The library-level run_impl_main tests above cover
// the same branches, but these prove the argument shape of the CLI
// still maps to the expected code paths.

fn run_start_lock(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("start-lock")
        .args(args)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap()
}

fn parse_cli(output: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last_line).unwrap_or_else(|_| serde_json::json!({"raw": stdout.trim()}))
}

#[test]
fn cli_acquire_missing_feature_exits_1() {
    let output = run_start_lock(&["--acquire"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_cli(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("feature required"),
        "error should mention missing feature, got: {}",
        msg
    );
}

#[test]
fn cli_release_missing_feature_exits_1() {
    let output = run_start_lock(&["--release"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_cli(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("feature required"),
        "error should mention missing feature, got: {}",
        msg
    );
}

#[test]
fn cli_no_flag_exits_1() {
    let output = run_start_lock(&[]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_cli(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("--acquire") || msg.contains("--release") || msg.contains("--check"),
        "error should mention valid flags, got: {}",
        msg
    );
}
