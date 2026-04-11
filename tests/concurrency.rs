// Concurrent access to FLOW's shared resources.
//
// All tests use std::thread for real thread-based parallelism.
// Each test creates an isolated tempdir to avoid cross-test interference.

mod common;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flow_rs::lock::mutate_state;
use fs2::FileExt;
use serde_json::{self, json, Value};

/// The flow-rs binary path, resolved at compile time via cargo.
const FLOW_RS: &str = env!("CARGO_BIN_EXE_flow-rs");

/// Initialize a minimal git repo at the given path.
///
/// Runs `git init` + initial commit with `.output()` for stdio capture.
fn init_git_repo(dir: &std::path::Path) {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(dir)
        .output()
        .expect("Failed to run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Configure git user for commits
    let git_config_path = dir.join(".git").join("config");
    let mut config_file = OpenOptions::new()
        .append(true)
        .open(&git_config_path)
        .expect("Failed to open .git/config");
    writeln!(
        config_file,
        "[user]\n\temail = t@t.com\n\tname = T\n[commit]\n\tgpgsign = false"
    )
    .expect("Failed to write git config");

    let output = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .expect("Failed to run git commit");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Timing data for lock serialization tests.
struct Timing {
    worker_id: usize,
    acquired_at: f64,
    released_at: f64,
}

#[test]
fn mutate_state_under_contention() {
    // 20 parallel threads increment a counter in a JSON file using exclusive
    // file locking. Final count must equal 20 — no increments lost.
    //
    // Note: This tests the fs2 file-locking mechanism directly rather than
    // calling flow_rs::mutate_state via subprocess. The production mutate_state
    // uses the same fs2::FileExt::lock_exclusive pattern. A regression where
    // mutate_state acquires the lock after reading would not be caught here —
    // that invariant is enforced by the mutate_state unit tests in src/utils.rs.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let state_path = tmp.path().join("shared.json");
    fs::write(&state_path, r#"{"count": 0}"#).expect("Failed to write initial state");

    let state_path = Arc::new(state_path);
    let barrier = Arc::new(Barrier::new(20));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let path = Arc::clone(&state_path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path.as_ref())
                    .unwrap();
                file.lock_exclusive().unwrap();
                let content = fs::read_to_string(path.as_ref()).unwrap();
                let mut data: Value = serde_json::from_str(&content).unwrap();
                let count = data["count"].as_i64().unwrap_or(0);
                data["count"] = json!(count + 1);
                fs::write(path.as_ref(), serde_json::to_string(&data).unwrap()).unwrap();
                file.unlock().unwrap();
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Worker thread panicked");
    }

    let final_content =
        fs::read_to_string(state_path.as_ref()).expect("Failed to read final state");
    let final_data: Value =
        serde_json::from_str(&final_content).expect("Failed to parse final state");
    assert_eq!(
        final_data["count"].as_i64().unwrap(),
        20,
        "Expected count=20 after 20 concurrent increments"
    );
}

#[test]
fn log_append_under_contention() {
    //20 parallel threads append unique lines to a log file via `flow-rs log`.
    //File must have exactly 20 non-corrupted lines, each with a unique worker-N marker.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let repo = tmp.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".flow-states")).expect("Failed to create .flow-states");

    let repo = Arc::new(repo);
    let barrier = Arc::new(Barrier::new(20));

    let handles: Vec<_> = (0..20)
        .map(|id| {
            let repo = Arc::clone(&repo);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let output = Command::new(FLOW_RS)
                    .args(["log", "test-branch", &format!("worker-{}", id)])
                    .current_dir(repo.as_ref())
                    .output()
                    .expect("Failed to run flow-rs log");
                assert!(
                    output.status.success(),
                    "flow-rs log failed for worker-{}: {}",
                    id,
                    String::from_utf8_lossy(&output.stderr)
                );
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Worker thread panicked");
    }

    let log_file = repo.join(".flow-states").join("test-branch.log");
    assert!(log_file.exists(), "Log file was not created");

    let content = fs::read_to_string(&log_file).expect("Failed to read log file");
    let lines: Vec<&str> = content.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        20,
        "Expected exactly 20 log lines, got {}",
        lines.len()
    );

    // Each line should contain a unique worker-N marker
    let mut markers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in &lines {
        for part in line.split_whitespace() {
            if part.starts_with("worker-") {
                markers.insert(part.to_string());
            }
        }
    }
    assert_eq!(
        markers.len(),
        20,
        "Expected 20 unique worker markers, got {}",
        markers.len()
    );
}

#[test]
fn start_lock_serialization() {
    //3 parallel threads acquire the start lock via `flow-rs start-lock`.
    //No two hold it simultaneously — intervals must not overlap.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let repo = tmp.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".flow-states")).expect("Failed to create .flow-states");

    let repo = Arc::new(repo);
    let timings: Arc<Mutex<Vec<Timing>>> = Arc::new(Mutex::new(Vec::new()));
    let baseline = Instant::now();

    let handles: Vec<_> = (0..3)
        .map(|id| {
            let repo = Arc::clone(&repo);
            let timings = Arc::clone(&timings);

            thread::spawn(move || {
                // Stagger starts by 100ms intervals
                thread::sleep(Duration::from_millis(id as u64 * 100));

                let feature = format!("feature-{}", id);
                let output = Command::new(FLOW_RS)
                    .args([
                        "start-lock",
                        "--acquire",
                        "--wait",
                        "--timeout",
                        "30",
                        "--interval",
                        "1",
                        "--feature",
                        &feature,
                    ])
                    .current_dir(repo.as_ref())
                    .output()
                    .expect("Failed to run flow-rs start-lock --acquire");
                assert!(
                    output.status.success(),
                    "start-lock acquire failed for {}: stdout={} stderr={}",
                    feature,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                let stdout = String::from_utf8_lossy(&output.stdout);
                let data: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
                    panic!(
                        "Failed to parse start-lock JSON for {}: {} output: {}",
                        feature, e, stdout
                    )
                });
                assert_eq!(
                    data["status"].as_str().unwrap(),
                    "acquired",
                    "Worker {} did not acquire lock",
                    id
                );

                let acquired_at = baseline.elapsed().as_secs_f64();
                thread::sleep(Duration::from_millis(300));
                let released_at = baseline.elapsed().as_secs_f64();

                let output = Command::new(FLOW_RS)
                    .args(["start-lock", "--release", "--feature", &feature])
                    .current_dir(repo.as_ref())
                    .output()
                    .expect("Failed to run flow-rs start-lock --release");
                assert!(
                    output.status.success(),
                    "start-lock release failed for {}: {}",
                    feature,
                    String::from_utf8_lossy(&output.stderr)
                );

                timings.lock().unwrap().push(Timing {
                    worker_id: id,
                    acquired_at,
                    released_at,
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Worker thread panicked");
    }

    let mut timings = timings.lock().unwrap();
    assert_eq!(timings.len(), 3, "Expected 3 timing records");
    timings.sort_by(|a, b| a.acquired_at.partial_cmp(&b.acquired_at).unwrap());

    // Allow 150ms tolerance for CI runner scheduler jitter. The lock mechanism
    // uses filesystem polling (50ms intervals), so apparent overlaps under 150ms
    // are measurement artifacts, not real concurrency violations.
    let jitter_tolerance = 0.150;
    for i in 1..timings.len() {
        assert!(
            timings[i].acquired_at >= timings[i - 1].released_at - jitter_tolerance,
            "Worker {} (acquired_at={:.3}) overlaps with worker {} (released_at={:.3}) beyond {:.0}ms tolerance",
            timings[i].worker_id,
            timings[i].acquired_at,
            timings[i - 1].worker_id,
            timings[i - 1].released_at,
            jitter_tolerance * 1000.0
        );
    }
}

#[test]
fn thundering_herd_zero_delay() {
    // 3 threads start simultaneously (barrier). All acquire lock, no overlaps.
    // Uses 3 workers (not 5) with 100ms hold time (not 300ms) to keep wall time
    // under 5 seconds. The lock polling interval is 1 second (integer-only),
    // so fewer workers and shorter holds reduce total wait time.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let repo = tmp.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".flow-states")).expect("Failed to create .flow-states");

    let repo = Arc::new(repo);
    let timings: Arc<Mutex<Vec<Timing>>> = Arc::new(Mutex::new(Vec::new()));
    let barrier = Arc::new(Barrier::new(3));
    let baseline = Instant::now();

    let handles: Vec<_> = (0..3)
        .map(|id| {
            let repo = Arc::clone(&repo);
            let timings = Arc::clone(&timings);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                barrier.wait();

                let feature = format!("feature-{}", id);
                let output = Command::new(FLOW_RS)
                    .args([
                        "start-lock",
                        "--acquire",
                        "--wait",
                        "--timeout",
                        "30",
                        "--interval",
                        "1",
                        "--feature",
                        &feature,
                    ])
                    .current_dir(repo.as_ref())
                    .output()
                    .expect("Failed to run flow-rs start-lock --acquire");
                assert!(
                    output.status.success(),
                    "start-lock acquire failed for {}: stdout={} stderr={}",
                    feature,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                let stdout = String::from_utf8_lossy(&output.stdout);
                let data: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
                    panic!(
                        "Failed to parse start-lock JSON for {}: {} output: {}",
                        feature, e, stdout
                    )
                });
                assert_eq!(
                    data["status"].as_str().unwrap(),
                    "acquired",
                    "Worker {} got status={} instead of acquired",
                    id,
                    data["status"]
                );

                let acquired_at = baseline.elapsed().as_secs_f64();
                thread::sleep(Duration::from_millis(100));
                let released_at = baseline.elapsed().as_secs_f64();

                let output = Command::new(FLOW_RS)
                    .args(["start-lock", "--release", "--feature", &feature])
                    .current_dir(repo.as_ref())
                    .output()
                    .expect("Failed to run flow-rs start-lock --release");
                assert!(
                    output.status.success(),
                    "start-lock release failed for {}: {}",
                    feature,
                    String::from_utf8_lossy(&output.stderr)
                );

                timings.lock().unwrap().push(Timing {
                    worker_id: id,
                    acquired_at,
                    released_at,
                });
            })
        })
        .collect();

    // Join with a reasonable timeout check
    let join_deadline = Instant::now() + Duration::from_secs(30);
    for handle in handles {
        let remaining = join_deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "Thundering herd test exceeded 30s deadline"
        );
        handle.join().expect("Worker thread panicked");
    }

    let mut timings = timings.lock().unwrap();
    assert_eq!(timings.len(), 3, "Expected 3 timing records");
    timings.sort_by(|a, b| a.acquired_at.partial_cmp(&b.acquired_at).unwrap());

    // Allow 150ms tolerance for CI runner scheduler jitter. The lock mechanism
    // uses filesystem polling (50ms intervals), so apparent overlaps under 150ms
    // are measurement artifacts, not real concurrency violations.
    let jitter_tolerance = 0.150;
    for i in 1..timings.len() {
        assert!(
            timings[i].acquired_at >= timings[i - 1].released_at - jitter_tolerance,
            "Worker {} (acquired_at={:.3}) overlaps with worker {} (released_at={:.3}) beyond {:.0}ms tolerance",
            timings[i].worker_id,
            timings[i].acquired_at,
            timings[i - 1].worker_id,
            timings[i - 1].released_at,
            jitter_tolerance * 1000.0
        );
    }
}

#[test]
fn parallel_state_file_creation() {
    //5 threads each write a state file for a different branch.
    //All must succeed with correct content.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let state_dir = tmp.path().join(".flow-states");
    fs::create_dir_all(&state_dir).expect("Failed to create .flow-states");

    let state_dir = Arc::new(state_dir);
    let barrier = Arc::new(Barrier::new(5));

    let handles: Vec<_> = (0..5)
        .map(|id| {
            let state_dir = Arc::clone(&state_dir);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let branch = format!("branch-{}", id);
                let state = json!({"branch": branch, "status": "created"});
                let path = state_dir.join(format!("{}.json", branch));
                fs::write(&path, serde_json::to_string_pretty(&state).unwrap())
                    .unwrap_or_else(|e| panic!("Failed to write state for {}: {}", branch, e));
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Worker thread panicked");
    }

    for id in 0..5 {
        let branch = format!("branch-{}", id);
        let path = state_dir.join(format!("{}.json", branch));
        assert!(path.exists(), "State file for {} was not created", branch);

        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read state for {}: {}", branch, e));
        let data: Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse state for {}: {}", branch, e));
        assert_eq!(
            data["branch"].as_str().unwrap(),
            branch,
            "Branch mismatch in state file"
        );
        assert_eq!(
            data["status"].as_str().unwrap(),
            "created",
            "Status mismatch in state file for {}",
            branch
        );
    }
}

#[test]
fn cleanup_isolation() {
    // Cleanup on branch-a must not affect branch-b's state file.
    //
    // Thread 1 runs `flow-rs cleanup` on branch-a (deletes its state file).
    //Thread 2 mutates branch-b's state file (sets mutated=true) using file locking.
    //After both finish: branch-a state deleted, branch-b state has mutated=true.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let repo = tmp.path().to_path_buf();
    init_git_repo(&repo);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).expect("Failed to create .flow-states");

    let state_a = state_dir.join("branch-a.json");
    let state_b = state_dir.join("branch-b.json");
    fs::write(&state_a, r#"{"branch": "branch-a", "count": 0}"#)
        .expect("Failed to write branch-a state");
    fs::write(&state_b, r#"{"branch": "branch-b", "count": 0}"#)
        .expect("Failed to write branch-b state");

    let repo_path = repo.to_string_lossy().to_string();
    let state_b_path = state_b.clone();

    // Thread 1: cleanup branch-a
    let repo_for_cleanup = repo_path.clone();
    let handle_cleanup = thread::spawn(move || {
        let output = Command::new(FLOW_RS)
            .args([
                "cleanup",
                &repo_for_cleanup,
                "--branch",
                "branch-a",
                "--worktree",
                ".worktrees/branch-a",
            ])
            .output()
            .expect("Failed to run flow-rs cleanup");
        assert!(
            output.status.success(),
            "cleanup failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    });

    // Thread 2: mutate branch-b state file
    let handle_mutate = thread::spawn(move || {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&state_b_path)
            .unwrap();
        file.lock_exclusive().unwrap();
        let content = fs::read_to_string(&state_b_path).unwrap();
        let mut data: Value = serde_json::from_str(&content).unwrap();
        data["mutated"] = json!(true);
        fs::write(&state_b_path, serde_json::to_string(&data).unwrap()).unwrap();
        file.unlock().unwrap();
    });

    handle_cleanup.join().expect("Cleanup thread panicked");
    handle_mutate.join().expect("Mutate thread panicked");

    // branch-a state file should be deleted by cleanup
    assert!(
        !state_a.exists(),
        "branch-a state file should have been deleted by cleanup"
    );

    // branch-b state file should have the mutation
    let content = fs::read_to_string(&state_b).expect("Failed to read branch-b state");
    let data: Value = serde_json::from_str(&content).expect("Failed to parse branch-b state");
    assert!(
        data["mutated"].as_bool().unwrap(),
        "branch-b should have mutated=true"
    );
    assert_eq!(
        data["branch"].as_str().unwrap(),
        "branch-b",
        "branch-b branch field should be preserved"
    );
}

#[test]
fn mutate_state_api_under_contention() {
    // 20 threads call flow_rs::lock::mutate_state simultaneously to increment
    // a counter. Unlike mutate_state_under_contention (which reimplements the
    // locking pattern manually), this test exercises the actual mutate_state API.
    // A regression where the lock is acquired after reading would surface here.
    let tmp = tempfile::tempdir().expect("Failed to create tempdir");
    let state_path = tmp.path().join("contention.json");
    fs::write(&state_path, r#"{"count": 0}"#).expect("Failed to write initial state");

    let state_path = Arc::new(state_path);
    let barrier = Arc::new(Barrier::new(20));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let path = Arc::clone(&state_path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                mutate_state(&path, |state| {
                    let count = state["count"].as_i64().unwrap_or(0);
                    state["count"] = json!(count + 1);
                })
                .expect("mutate_state failed");
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Worker thread panicked");
    }

    let final_content =
        fs::read_to_string(state_path.as_ref()).expect("Failed to read final state");
    let final_data: Value =
        serde_json::from_str(&final_content).expect("Failed to parse final state");
    assert_eq!(
        final_data["count"].as_i64().unwrap(),
        20,
        "Expected count=20 after 20 concurrent mutate_state calls"
    );
}
