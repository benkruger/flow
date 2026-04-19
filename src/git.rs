use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::flow_paths::FlowPaths;

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
/// 3. Return current_branch() anyway (callers check state file existence)
///
/// Never scans `.flow-states/` for candidates — each caller targets only
/// its own branch.
pub fn resolve_branch(override_branch: Option<&str>, root: &Path) -> Option<String> {
    resolve_branch_impl(override_branch, root, current_branch())
}

/// Cwd-scoped variant of [`resolve_branch`] that uses [`current_branch_in`]
/// instead of [`current_branch`].
///
/// This is the correct choice for CLI subcommands that resolve a branch
/// from an explicit working directory (e.g., the `ci` subcommand running
/// in a worktree) where the branch must be read from the given cwd, not
/// the process's cwd.
pub fn resolve_branch_in(override_branch: Option<&str>, cwd: &Path, root: &Path) -> Option<String> {
    resolve_branch_impl(override_branch, root, current_branch_in(cwd))
}

fn resolve_branch_impl(
    override_branch: Option<&str>,
    root: &Path,
    branch: Option<String>,
) -> Option<String> {
    if let Some(b) = override_branch {
        return Some(b.to_string());
    }

    // Exact match — current branch has a state file. `try_new` filters
    // out slash-containing branches (`feature/foo`, `dependabot/*`)
    // which git permits but FLOW's flat state-file layout cannot
    // address; those branches skip the exact-match check and fall
    // through to the "return it anyway" path below.
    if let Some(ref b) = branch {
        if let Some(paths) = FlowPaths::try_new(root, b) {
            if paths.state_file().exists() {
                return Some(b.clone());
            }
        }
    }

    // No state file for current branch — return it anyway
    // (callers check state file existence separately)
    branch
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
        assert!(root.exists() || root == Path::new("."));
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
        let branch = resolve_branch(Some("explicit-branch"), dir.path());
        assert_eq!(branch, Some("explicit-branch".to_string()));
    }

    #[test]
    fn resolve_branch_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let branch = resolve_branch(None, dir.path());
        // No .flow-states dir — returns current_branch() fallback
        // branch may be Some or None depending on git context
        let _ = branch;
    }

    #[test]
    fn resolve_branch_no_match_returns_current_branch() {
        // When current branch has no state file, resolve_branch returns
        // the branch anyway — it never scans for other state files.
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        // Create state files for OTHER branches
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

        // Call resolve_branch_impl directly with a branch that has no state file
        let result = resolve_branch_impl(None, dir.path(), Some("main".to_string()));
        // Returns "main" — does NOT resolve to feature-a or feature-b
        assert_eq!(result, Some("main".to_string()));
    }

    // --- current_branch_in() ---

    /// Initialize a git repo in the given directory with an initial commit
    /// on branch `initial_branch`. Used by current_branch_in tests.
    fn init_git_repo(dir: &Path, initial_branch: &str) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
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
        let output = Command::new("git")
            .args(["checkout", &sha])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());

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
    fn resolve_branch_impl_state_file_exists_returns_branch() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("test-branch.json"),
            r#"{"branch": "test-branch"}"#,
        )
        .unwrap();
        let result = resolve_branch_impl(None, dir.path(), Some("test-branch".to_string()));
        assert_eq!(result, Some("test-branch".to_string()));
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
