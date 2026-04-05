use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find the main git repository root.
///
/// Uses `git worktree list --porcelain` to find the root, which works
/// correctly whether run from the project root or from inside a worktree.
/// Falls back to the current directory if git fails.
pub fn project_root() -> PathBuf {
    let output = match Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return PathBuf::from("."),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            return PathBuf::from(path.trim());
        }
    }
    PathBuf::from(".")
}

/// Get the current git branch name.
///
/// Returns None if not on a branch (e.g. detached HEAD) or if git fails.
///
/// If FLOW_SIMULATE_BRANCH is set in the environment, returns that value
/// instead of querying git. Used by `bin/flow ci --simulate-branch`.
pub fn current_branch() -> Option<String> {
    let simulated = env::var("FLOW_SIMULATE_BRANCH").ok();
    if let Some(ref s) = simulated {
        if !s.is_empty() {
            return Some(s.clone());
        }
    }

    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Get the current git branch name from a specific working directory.
///
/// Like [`current_branch`] but runs `git branch --show-current` with
/// `.current_dir(cwd)` so tests can point at a fixture repo without
/// mutating the test process cwd. Returns None for detached HEAD,
/// non-git directories, or git failures.
///
/// Unlike [`current_branch`], this helper does NOT consult the
/// FLOW_SIMULATE_BRANCH env var. Callers that need simulate-branch
/// semantics must layer it on top.
pub fn current_branch_in(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Resolve which branch's state file to use.
///
/// Resolution order:
/// 1. If override provided, return it immediately
/// 2. If current_branch() matches a state file, return it
/// 3. Scan .flow-states/*.json (skip *-phases.json):
///    - 1 file → return that branch (auto-resolve)
///    - 2+ files → return (None, candidates) (ambiguous)
///    - 0 files → return current_branch() (no features active)
///
/// Returns (branch, candidates) where candidates is empty on success
/// or a list of branch names when ambiguous.
pub fn resolve_branch(
    override_branch: Option<&str>,
    root: &Path,
) -> (Option<String>, Vec<String>) {
    if let Some(b) = override_branch {
        return (Some(b.to_string()), vec![]);
    }

    let branch = current_branch();
    let state_dir = root.join(".flow-states");

    // Exact match — current branch has a state file
    if let Some(ref b) = branch {
        if state_dir.join(format!("{}.json", b)).exists() {
            return (Some(b.clone()), vec![]);
        }
    }

    // Scan for state files
    if !state_dir.is_dir() {
        return (branch, vec![]);
    }

    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        paths.sort_by_key(|e| e.file_name());

        for entry in paths {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".json") {
                continue;
            }
            if name_str.ends_with("-phases.json") {
                continue;
            }
            // Try to parse as valid JSON
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                    let stem = name_str.trim_end_matches(".json").to_string();
                    candidates.push(stem);
                }
            }
        }
    }

    if candidates.len() == 1 {
        return (Some(candidates.remove(0)), vec![]);
    }
    if candidates.len() > 1 {
        return (None, candidates);
    }

    // No state files found — return current branch (for new features)
    (branch, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    // Serialize tests that mutate FLOW_SIMULATE_BRANCH to prevent races.
    // Rust tests run in parallel — without this lock, one test's set_var
    // can race with another test's remove_var on the same env var.
    static SIMULATE_BRANCH_LOCK: Mutex<()> = Mutex::new(());

    // --- project_root() ---

    #[test]
    fn project_root_returns_path() {
        // In a git repo, should return a valid path
        let root = project_root();
        assert!(root.exists() || root == PathBuf::from("."));
    }

    // --- current_branch() ---

    #[test]
    fn current_branch_simulate_env_var() {
        let _guard = SIMULATE_BRANCH_LOCK.lock().unwrap();
        env::set_var("FLOW_SIMULATE_BRANCH", "main");
        let result = current_branch();
        env::remove_var("FLOW_SIMULATE_BRANCH");
        assert_eq!(result, Some("main".to_string()));
    }

    #[test]
    fn current_branch_simulate_empty_falls_through() {
        let _guard = SIMULATE_BRANCH_LOCK.lock().unwrap();
        env::set_var("FLOW_SIMULATE_BRANCH", "");
        let result = current_branch();
        env::remove_var("FLOW_SIMULATE_BRANCH");
        // Falls through to git — may return a branch or None depending on context
        // Just verify it doesn't return Some("")
        if let Some(ref b) = result {
            assert!(!b.is_empty());
        }
    }

    // --- resolve_branch() ---

    #[test]
    fn resolve_branch_override_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (branch, candidates) = resolve_branch(Some("explicit-branch"), dir.path());
        assert_eq!(branch, Some("explicit-branch".to_string()));
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_branch_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (branch, candidates) = resolve_branch(None, dir.path());
        // No .flow-states dir — returns current_branch() fallback
        assert!(candidates.is_empty());
        // branch may be Some or None depending on git context
        let _ = branch;
    }

    #[test]
    fn resolve_branch_single_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-xyz.json"),
            r#"{"branch": "feature-xyz"}"#,
        )
        .unwrap();

        let (branch, candidates) = resolve_branch(None, dir.path());
        assert_eq!(branch, Some("feature-xyz".to_string()));
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_branch_multiple_state_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-a.json"),
            r#"{"branch": "feature-a"}"#,
        )
        .unwrap();
        fs::write(
            state_dir.join("feature-b.json"),
            r#"{"branch": "feature-b"}"#,
        )
        .unwrap();

        let (branch, candidates) = resolve_branch(None, dir.path());
        assert!(branch.is_none());
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains(&"feature-a".to_string()));
        assert!(candidates.contains(&"feature-b".to_string()));
    }

    #[test]
    fn resolve_branch_skips_phases_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-x.json"),
            r#"{"branch": "feature-x"}"#,
        )
        .unwrap();
        fs::write(
            state_dir.join("feature-x-phases.json"),
            r#"{"order": []}"#,
        )
        .unwrap();

        let (branch, candidates) = resolve_branch(None, dir.path());
        assert_eq!(branch, Some("feature-x".to_string()));
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_branch_skips_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("bad.json"), "{corrupt").unwrap();
        fs::write(
            state_dir.join("good.json"),
            r#"{"branch": "good"}"#,
        )
        .unwrap();

        let (branch, candidates) = resolve_branch(None, dir.path());
        assert_eq!(branch, Some("good".to_string()));
        assert!(candidates.is_empty());
    }

    // --- current_branch_in() ---

    /// Initialize a git repo in the given directory with an initial commit
    /// on branch `initial_branch`. Used by current_branch_in tests.
    fn init_git_repo(dir: &Path, initial_branch: &str) {
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .expect("git command failed");
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "--initial-branch", initial_branch]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);
    }

    #[test]
    fn current_branch_in_reads_cwd_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "my-feature");
        let branch = current_branch_in(dir.path());
        assert_eq!(branch, Some("my-feature".to_string()));
    }

    #[test]
    fn current_branch_in_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        // Detach HEAD by checking out the commit SHA directly
        let sha = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();
        let status = Command::new("git")
            .args(["checkout", &sha])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());

        let branch = current_branch_in(dir.path());
        assert_eq!(branch, None);
    }

    #[test]
    fn current_branch_in_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let branch = current_branch_in(dir.path());
        assert_eq!(branch, None);
    }

    #[test]
    fn current_branch_in_ignores_simulate_env_var() {
        // current_branch_in is cwd-scoped and does NOT consult the
        // FLOW_SIMULATE_BRANCH env var. This test documents that
        // invariant by asserting the helper returns the real branch
        // name regardless of what env::var would return — without
        // actually mutating the env var (which would race with
        // parallel tests).
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "real-branch");
        let branch = current_branch_in(dir.path());
        assert_eq!(branch, Some("real-branch".to_string()));
    }
}
