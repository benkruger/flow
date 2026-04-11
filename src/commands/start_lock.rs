//! Queue-based start lock serialization.
//!
//! Prevents concurrent starts from fighting over main (CI fixes, dependency
//! updates). Only one flow-start runs at a time. The oldest queue entry
//! (by mtime, then feature name) holds the lock.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::git::project_root;
use crate::output::json_error;

pub const QUEUE_DIRNAME: &str = "start-queue";
pub const STALE_TIMEOUT_SECONDS: u64 = 1800; // 30 minutes

/// Get file mtime as seconds since UNIX epoch.
fn mtime_secs(path: &Path) -> Option<f64> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let duration = mtime.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_secs_f64())
}

/// Create the queue directory if needed, return its path.
pub fn queue_path(root: &Path) -> PathBuf {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let state_dir = root.join(".flow-states");
    let _ = fs::create_dir_all(&state_dir);
    let queue_dir = state_dir.join(QUEUE_DIRNAME);
    let _ = fs::create_dir_all(&queue_dir);
    queue_dir
}

/// List queue entries sorted by (mtime_secs, name).
///
/// Stale entries (older than STALE_TIMEOUT_SECONDS) are always excluded
/// from the returned list. If `cleanup` is true, they are also deleted.
/// Returns (entries, stale_found) where stale_found indicates whether
/// any stale entries were encountered.
pub fn list_queue(queue_dir: &Path, cleanup: bool) -> (Vec<(f64, String)>, bool) {
    let mut stale_found = false;
    let mut entries: Vec<(f64, String)> = Vec::new();

    let items = match fs::read_dir(queue_dir) {
        Ok(items) => items,
        Err(_) => return (entries, false),
    };

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    for entry in items.flatten() {
        let path = entry.path();
        // Skip non-files (directories, symlinks, etc.)
        if !path.is_file() {
            continue;
        }
        let mtime = match mtime_secs(&path) {
            Some(t) => t,
            None => continue, // stat failed — skip
        };
        if (now_secs - mtime) > STALE_TIMEOUT_SECONDS as f64 {
            if cleanup {
                let _ = fs::remove_file(&path);
            }
            stale_found = true;
            continue; // stale entries excluded from list
        }
        let name = entry.file_name().to_string_lossy().to_string();
        entries.push((mtime, name));
    }

    entries.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    (entries, stale_found)
}

/// Attempt to acquire the start lock via the queue.
///
/// Creates a queue entry for this feature. If we are first in queue
/// (by mtime, then name), returns acquired. Otherwise returns locked
/// with the current holder's name.
pub fn acquire(feature: &str, queue_dir: &Path) -> Value {
    let lock_path = queue_dir.display().to_string();
    let entry = queue_dir.join(feature);

    // Create our queue entry only if it doesn't exist yet.
    // If an existing entry is stale (from a previous incomplete run),
    // replace it so _list_queue cleanup doesn't delete our only entry.
    if entry.exists() {
        if let Some(mtime) = mtime_secs(&entry) {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            if (now_secs - mtime) > STALE_TIMEOUT_SECONDS as f64 {
                let _ = fs::remove_file(&entry);
                let _ = fs::File::create(&entry);
            }
        }
    } else {
        let _ = fs::File::create(&entry);
    }

    // List queue with stale cleanup
    let (entries, stale_removed) = list_queue(queue_dir, true);

    if entries.is_empty() {
        // Should not happen — we just created our entry
        let mut result = json!({"status": "acquired", "lock_path": lock_path});
        if stale_removed {
            result["stale_broken"] = json!(true);
        }
        return result;
    }

    let holder = &entries[0].1;
    if holder == feature {
        let mut result = json!({"status": "acquired", "lock_path": lock_path});
        if stale_removed {
            result["stale_broken"] = json!(true);
        }
        result
    } else {
        let mut result = json!({
            "status": "locked",
            "feature": holder.clone(),
            "lock_path": lock_path,
        });
        if stale_removed {
            result["stale_broken"] = json!(true);
        }
        result
    }
}

/// Acquire with retry loop using the real thread::sleep.
pub fn acquire_with_wait(feature: &str, queue_dir: &Path, timeout: u64, interval: u64) -> Value {
    acquire_with_wait_impl(feature, queue_dir, timeout, interval, |d| {
        std::thread::sleep(d)
    })
}

/// Internal: acquire with injectable sleep for testing.
fn acquire_with_wait_impl<F>(
    feature: &str,
    queue_dir: &Path,
    timeout: u64,
    interval: u64,
    mut sleep_fn: F,
) -> Value
where
    F: FnMut(Duration),
{
    let start = std::time::Instant::now();
    let result = acquire(feature, queue_dir);
    if result["status"] == "acquired" {
        return result;
    }

    loop {
        let elapsed = start.elapsed().as_secs();
        if elapsed >= timeout {
            return json!({
                "status": "timeout",
                "feature": result["feature"],
                "waited_seconds": elapsed as i64,
                "lock_path": result["lock_path"],
            });
        }
        let remaining = timeout - elapsed;
        sleep_fn(Duration::from_secs(std::cmp::min(interval, remaining)));
        let result = acquire(feature, queue_dir);
        if result["status"] == "acquired" {
            return result;
        }
    }
}

/// Release the start lock by removing the queue entry.
///
/// Returns `was_present: true` if the file existed before removal,
/// `false` if it was already absent. Status is `"released"` in both
/// cases (idempotent). `"error"` only if the file persists after unlink.
pub fn release(feature: &str, queue_dir: &Path) -> Value {
    let lock_path = queue_dir.display().to_string();
    let entry = queue_dir.join(feature);
    let was_present = entry.exists();
    let _ = fs::remove_file(&entry);

    if entry.exists() {
        return json!({
            "status": "error",
            "message": "Queue entry persists after unlink",
            "lock_path": lock_path,
            "was_present": true,
        });
    }

    json!({"status": "released", "lock_path": lock_path, "was_present": was_present})
}

/// Check lock status without modifying.
pub fn check(queue_dir: &Path) -> Value {
    let lock_path = queue_dir.display().to_string();
    let (entries, _) = list_queue(queue_dir, false);

    if entries.is_empty() {
        return json!({"status": "free", "lock_path": lock_path});
    }

    let holder = &entries[0].1;
    json!({
        "status": "locked",
        "feature": holder.clone(),
        "lock_path": lock_path,
    })
}

/// CLI entry point.
pub fn run(
    acquire_flag: bool,
    release_flag: bool,
    check_flag: bool,
    feature: Option<String>,
    wait: bool,
    timeout: u64,
    interval: u64,
) {
    if acquire_flag {
        let feature = match feature {
            Some(f) => f,
            None => {
                json_error("--feature required for --acquire", &[]);
                process::exit(1);
            }
        };
        let root = project_root();
        let queue_dir = queue_path(&root);
        let result = if wait {
            acquire_with_wait(&feature, &queue_dir, timeout, interval)
        } else {
            acquire(&feature, &queue_dir)
        };
        println!("{}", serde_json::to_string(&result).unwrap());
    } else if release_flag {
        let feature = match feature {
            Some(f) => f,
            None => {
                json_error("--feature required for --release", &[]);
                process::exit(1);
            }
        };
        let root = project_root();
        let queue_dir = queue_path(&root);
        let result = release(&feature, &queue_dir);
        println!("{}", serde_json::to_string(&result).unwrap());
    } else if check_flag {
        let root = project_root();
        let queue_dir = queue_path(&root);
        let result = check(&queue_dir);
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        json_error("Specify --acquire, --release, or --check", &[]);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};

    // --- queue_path tests ---

    #[test]
    fn test_queue_path_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let qp = queue_path(&root);
        assert!(qp.is_dir());
        assert_eq!(qp, root.join(".flow-states").join(QUEUE_DIRNAME));
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
        assert_eq!(entries[0].1, "alpha"); // alphabetically first
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
        assert!(stale_flag); // stale detected
        assert!(entries.is_empty()); // stale entries excluded from list
        assert!(stale.exists()); // file still present (not cleaned up)
    }

    #[test]
    fn test_list_queue_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let queue_dir = dir.path().join("nonexistent");
        let (entries, stale) = list_queue(&queue_dir, false);
        assert!(entries.is_empty());
        assert!(!stale);
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
        set_file_mtime(
            &stale_self,
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(1860)),
        )
        .unwrap();

        let result = acquire("my-feature", queue_dir);
        assert_eq!(result["status"], "acquired");
        // Entry must still exist (replaced with fresh mtime, not deleted)
        assert!(queue_dir.join("my-feature").exists());
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

    // --- acquire_with_wait tests ---

    #[test]
    fn test_acquire_with_wait_immediate() {
        let dir = tempfile::tempdir().unwrap();
        let queue_dir = dir.path();

        let mut sleep_called = false;
        let result = acquire_with_wait_impl("test-feature", queue_dir, 90, 10, |_| {
            sleep_called = true;
        });
        assert_eq!(result["status"], "acquired");
        assert!(!sleep_called);
    }

    #[test]
    fn test_acquire_with_wait_succeeds_after_retry() {
        let dir = tempfile::tempdir().unwrap();
        let queue_dir = dir.path();

        let blocking = queue_dir.join("blocking-feature");
        fs::write(&blocking, "").unwrap();
        set_file_mtime(
            &blocking,
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
        )
        .unwrap();

        let blocking_clone = blocking.clone();
        let result = acquire_with_wait_impl("new-feature", queue_dir, 10, 1, move |_| {
            let _ = fs::remove_file(&blocking_clone);
        });
        assert_eq!(result["status"], "acquired");
    }

    #[test]
    fn test_acquire_with_wait_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let queue_dir = dir.path();

        let blocking = queue_dir.join("blocking-feature");
        fs::write(&blocking, "").unwrap();
        set_file_mtime(
            &blocking,
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
        )
        .unwrap();

        let result = acquire_with_wait_impl(
            "new-feature",
            queue_dir,
            0, // timeout=0 triggers immediate timeout after first attempt
            10,
            |_| {},
        );
        assert_eq!(result["status"], "timeout");
        assert_eq!(result["feature"], "blocking-feature");
        assert!(result["waited_seconds"].is_number());
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
        // Stale entries excluded from list but not deleted
        assert_eq!(result["status"], "free");
        assert!(stale.exists());
    }

    #[test]
    fn test_list_queue_future_mtime_not_stale() {
        // A queue entry with mtime in the future (clock skew) must not be
        // classified as stale. The stale check computes (now - mtime); a future
        // mtime produces a negative value which is always < STALE_TIMEOUT_SECONDS.
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
        assert_eq!(entries.len(), 1, "Future-mtime entry should be in the list");
        assert_eq!(entries[0].1, "future-feature");
        assert!(
            !stale_found,
            "Future mtime should not trigger stale detection"
        );
        assert!(
            future_entry.exists(),
            "Future-mtime entry must not be deleted"
        );
    }
}
