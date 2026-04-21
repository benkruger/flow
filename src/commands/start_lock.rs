//! Queue-based start lock serialization.
//!
//! Prevents concurrent starts from fighting over main (CI fixes, dependency
//! updates). Only one flow-start runs at a time. The oldest queue entry
//! (by mtime, then feature name) holds the lock.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::flow_paths::FlowStatesDir;

pub const QUEUE_DIRNAME: &str = "start-queue";
pub const STALE_TIMEOUT_SECONDS: u64 = 1800; // 30 minutes

/// Get file mtime as seconds since UNIX epoch. Returns `0.0` on any
/// failure (metadata error, mtime unsupported, or a pre-UNIX_EPOCH
/// mtime). Callers treat `0.0` as maximally stale, so a broken entry
/// is classified as stale and cleaned up on the next list_queue pass
/// — the same outcome we'd get from an explicit skip branch, but
/// expressed without a fallible return type.
fn mtime_secs(path: &Path) -> f64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        })
        .unwrap_or(0.0)
}

/// Create the queue directory if needed, return its path.
pub fn queue_path(root: &Path) -> PathBuf {
    let root = match root.canonicalize() {
        Ok(p) => p,
        Err(_) => root.to_path_buf(),
    };
    // The queue lives under `.flow-states/` and is shared across every
    // branch on this machine, so FlowStatesDir (branch-free) is the
    // right address for it.
    let state_dir = FlowStatesDir::new(&root).path().to_path_buf();
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
        let mtime = mtime_secs(&path);
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
        let mtime = mtime_secs(&entry);
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        if (now_secs - mtime) > STALE_TIMEOUT_SECONDS as f64 {
            let _ = fs::remove_file(&entry);
            let _ = fs::File::create(&entry);
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
    acquire_with_wait_impl(
        feature,
        queue_dir,
        timeout,
        interval,
        &mut |d| std::thread::sleep(d),
    )
}

/// Seam-injected variant of [`acquire_with_wait`] that accepts a
/// custom sleep closure. Tests substitute a no-op or side-effecting
/// closure to drive every branch without blocking on real time.
///
/// Takes `&mut dyn FnMut(Duration)` rather than a generic `F`
/// parameter so every caller's closure compiles into the SAME
/// monomorphization. Generics create one monomorphization per
/// caller type, and callers from other test binaries (or from
/// code paths that never execute in the per-file test binary)
/// show up as `Unexecuted instantiation`, inflating the
/// uncovered-region/line counts. See
/// `.claude/rules/rust-patterns.md` "Seam-injection variant for
/// externally-coupled code" and the coverage-quest prompt's
/// "Generic functions" note.
pub fn acquire_with_wait_impl(
    feature: &str,
    queue_dir: &Path,
    timeout: u64,
    interval: u64,
    sleep_fn: &mut dyn FnMut(Duration),
) -> Value {
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

/// Testable core of the start-lock CLI. Returns the JSON payload the
/// CLI wrapper would print plus the exit code. The wrapper in
/// `main.rs` computes `project_root()` once and passes it in.
pub fn run_impl_main(
    acquire_flag: bool,
    release_flag: bool,
    check_flag: bool,
    feature: Option<String>,
    wait: bool,
    timeout: u64,
    interval: u64,
    root: &Path,
) -> (Value, i32) {
    if acquire_flag {
        let feature = match feature {
            Some(f) => f,
            None => {
                return (
                    json!({"status": "error", "message": "--feature required for --acquire"}),
                    1,
                )
            }
        };
        let queue_dir = queue_path(root);
        let result = if wait {
            acquire_with_wait(&feature, &queue_dir, timeout, interval)
        } else {
            acquire(&feature, &queue_dir)
        };
        (result, 0)
    } else if release_flag {
        let feature = match feature {
            Some(f) => f,
            None => {
                return (
                    json!({"status": "error", "message": "--feature required for --release"}),
                    1,
                )
            }
        };
        let queue_dir = queue_path(root);
        (release(&feature, &queue_dir), 0)
    } else if check_flag {
        let queue_dir = queue_path(root);
        (check(&queue_dir), 0)
    } else {
        (
            json!({"status": "error", "message": "Specify --acquire, --release, or --check"}),
            1,
        )
    }
}
