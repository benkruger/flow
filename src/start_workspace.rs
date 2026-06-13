//! Consolidated start-workspace: worktree creation + PR creation + state
//! backfill + Flow In-Progress label apply + lock release in a single
//! command.
//!
//! Lock is released as the final action (even on error), closing the
//! race condition where another flow could commit to main between
//! lock release and worktree creation. The label apply runs only on
//! the success path — AFTER worktree, PR, and state backfill all
//! succeed and BEFORE the lock release — so the Flow In-Progress
//! label means "a flow is live, worktree exists, PR exists" rather
//! than "a flow was attempted". Failure paths skip the label apply
//! entirely; a failed start-workspace leaves no sticky label that
//! blocks the next retry.
//!
//! Worktree creation also mirrors every `.venv` and `node_modules`
//! directory found under the project root into the new worktree as
//! relative symlinks. The walker discovers each target at any depth
//! (root, mono-repo subdirs like `cortex/.venv` or
//! `cortex/frontend/node_modules`, deeply-nested layouts like
//! `packages/api/.venv`), skips dotted directories other than the
//! target itself plus a small named-skip list (`node_modules`,
//! `target`, `vendor`, `build`, `dist`), does not follow directory
//! symlinks (cycle protection), and never overwrites pre-existing
//! committed content at a link path. The target-name match runs
//! BEFORE the skip filter, so mirroring `node_modules` is safe even
//! though the same name appears in the skip list — the match arm
//! fires and `continue`s before the skip check is reached.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use std::time::Duration;

use crate::commands::log::append_log;
use crate::commands::start_lock::{queue_path, release};
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::github::detect_repo;
use crate::label_issues::label_issues;
use crate::lock::mutate_state;
use crate::utils::{derive_feature, extract_issue_numbers, run_cmd, SetupError};

#[derive(Parser, Debug)]
#[command(
    name = "start-workspace",
    about = "Create worktree, PR, backfill state, release lock"
)]
pub struct Args {
    /// Human-readable feature description (for fallback prompt text)
    pub description: String,

    /// Canonical branch name (from init-state)
    #[arg(long)]
    pub branch: String,

    /// Path to file containing start prompt
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,
}

/// Extract PR number from URL like https://github.com/org/repo/pull/123.
///
/// Searches for the "pull" segment and parses the next segment as the number.
/// Returns 0 if the URL is malformed or not a PR URL.
fn extract_pr_number(pr_url: &str) -> u32 {
    pr_url
        .trim_end_matches('/')
        .split('/')
        .skip_while(|s| *s != "pull")
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Recursively scan `root` for directories named `target` and return
/// each one's root-relative parent path. An empty `PathBuf` represents
/// a root-level match. Skips, on the recursive-descent branch:
/// directory symlinks (cycle protection), dotted directories other
/// than the `target` itself (`.git`, `.next`, `.gradle`,
/// `.pytest_cache`, `.tox`, etc.), and a small named-skip list
/// (`node_modules`, `target`, `vendor`, `build`, `dist`) that would
/// bloat the walk. The target-name match runs BEFORE the skip
/// filter, so a target like `node_modules` is mechanically safe to
/// mirror even though the same name appears in the skip list — the
/// match arm fires and `continue`s before the skip check is reached.
/// Does not recurse INTO a found target directory.
///
/// Symlink handling is asymmetric by design: the recursive descent
/// guard `!path.is_symlink()` prevents the walker from following
/// directory symlinks (cycle protection), but the target-detection
/// branch uses `path.is_dir()` which DOES follow symlinks. A
/// `target` entry that is itself a symlink to a real directory is
/// therefore discovered and mirrored — pnpm/yarn-workspace setups
/// where `node_modules` is a symlink to a shared store get the same
/// mirroring as inline directories. The symlink guard scopes only
/// to recursive descent; target detection at the current entry
/// always follows.
fn find_dep_parents(root: &Path, target: &str) -> Vec<PathBuf> {
    const SKIP_NAMED: &[&str] = &["node_modules", "target", "vendor", "build", "dist"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name_owned = entry.file_name();
            let name = name_owned.to_string_lossy();
            let name_ref: &str = name.as_ref();
            if name_ref == target {
                if path.is_dir() {
                    let parent = path
                        .parent()
                        .expect("entry path under root always has a parent");
                    let rel = parent
                        .strip_prefix(root)
                        .expect("entry path is a descendant of root");
                    out.push(rel.to_path_buf());
                }
                continue;
            }
            if SKIP_NAMED.contains(&name_ref) || name_ref.starts_with('.') {
                continue;
            }
            if !path.is_symlink() && path.is_dir() {
                stack.push(path);
            }
        }
    }
    out
}

/// Build the relative symlink target a worktree-side `target` link
/// (e.g. `.venv` or `node_modules`) should point at, given the
/// source-side parent's root-relative path. The worktree lives at
/// `<root>/.worktrees/<branch>/`, and the link sits at
/// `<root>/.worktrees/<branch>/<parent>/<target>`. The target uses
/// `..` components to escape `.worktrees/<branch>/<parent>/` back to
/// `<root>/<parent>/<target>` — `depth + 2` components: two for
/// `.worktrees/<branch>/`, one per segment of `parent`.
///
/// Examples by depth (target = `.venv`):
///
/// - depth 0 (`parent_relpath` empty, root-level `.venv`): `../../.venv`
/// - depth 1 (`cortex`): `../../../cortex/.venv`
/// - depth 2 (`packages/api`): `../../../../packages/api/.venv`
///
/// Examples by depth (target = `node_modules`):
///
/// - depth 0 (root-level `node_modules`): `../../node_modules`
/// - depth 1 (`web`): `../../../web/node_modules`
/// - depth 2 (`cortex/frontend`): `../../../../cortex/frontend/node_modules`
fn relative_dep_target(parent_relpath: &Path, target: &str) -> PathBuf {
    let depth = parent_relpath.components().count();
    let mut up = PathBuf::new();
    for _ in 0..(depth + 2) {
        up.push("..");
    }
    if parent_relpath.as_os_str().is_empty() {
        up.join(target)
    } else {
        up.join(parent_relpath).join(target)
    }
}

/// Walk the source tree under `root` and create relative symlinks
/// at the corresponding paths under `wt_path` for every directory
/// named `target` discovered (e.g. `.venv` or `node_modules`).
/// Best-effort: every IO error in the loop is swallowed so a single
/// partial failure (permission error, filesystem conflict) does not
/// abort the whole worktree-creation step. Pre-existing entries at
/// the link path are skipped via `fs::symlink_metadata().is_ok()`
/// per `.claude/rules/rust-patterns.md` "Symlink-Safe Existence
/// Checks Before Writes" — committed content the worktree's branch
/// already carries is preserved, never overwritten.
fn link_deps(root: &Path, wt_path: &Path, target: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        for parent in find_dep_parents(root, target) {
            let link = wt_path.join(&parent).join(target);
            if std::fs::symlink_metadata(&link).is_ok() {
                continue;
            }
            let link_parent = link
                .parent()
                .expect("link path is wt/<parent>/<target>; parent always exists");
            let _ = std::fs::create_dir_all(link_parent);
            let target_path = relative_dep_target(&parent, target);
            let _ = symlink(&target_path, &link);
        }
    }
}

/// Create a git worktree for the feature branch and mirror every
/// `.venv` and `node_modules` directory discovered under the project
/// root into the worktree as a relative symlink. Mirroring is
/// best-effort and preserves any pre-existing committed content at a
/// link path — see [`link_deps`] for the policy.
pub(crate) fn create_worktree(
    project_root: &std::path::Path,
    branch: &str,
) -> Result<PathBuf, SetupError> {
    let wt_path = project_root.join(".worktrees").join(branch);
    run_cmd(
        &[
            "git",
            "worktree",
            "add",
            &wt_path.to_string_lossy(),
            "-b",
            branch,
        ],
        project_root,
        "worktree",
        None,
    )?;

    link_deps(project_root, &wt_path, ".venv");
    link_deps(project_root, &wt_path, "node_modules");

    Ok(wt_path)
}

/// Make empty commit, push, and create PR. Returns (pr_url, pr_number).
///
/// `root` — the project root (main repo). The bootstrap empty commit
/// runs in the worktree, so its message file is
/// `<wt_path>/.flow-commit-msg` (the same convention `finalize-commit`
/// derives from its commit cwd). The file is untracked, gitignored via
/// `EXCLUDE_ENTRIES`, and removed after the commit.
///
/// `base_branch` — the integration branch to target as the PR's base.
/// Read from the state file by [`run_impl_with_paths`]; written there
/// at flow-start by [`crate::commands::init_state`] via
/// [`crate::git::current_branch_in`] so it equals `git branch --show-current`
/// at the moment `/flow:flow-start` was invoked.
pub(crate) fn initial_commit_push_pr(
    wt_path: &std::path::Path,
    branch: &str,
    feature_title: &str,
    prompt: &str,
    base_branch: &str,
) -> Result<(String, u32), SetupError> {
    // The bootstrap empty commit runs in the worktree (`wt_path`), so
    // its message file is `<wt_path>/.flow-commit-msg` — the same
    // convention `finalize-commit` derives from its commit cwd. The
    // file is untracked and gitignored via `EXCLUDE_ENTRIES`, removed
    // after the commit below.
    let commit_msg_path = wt_path.join(".flow-commit-msg");
    // The worktree directory exists (git just created it via
    // `worktree add`). A failure here would indicate disk-full or a
    // read-only filesystem — neither is a FLOW-supported recovery
    // state, so treat as an invariant via `.expect()`.
    std::fs::write(&commit_msg_path, format!("Start {} branch", branch))
        .expect("commit-msg write must succeed in the freshly-created worktree");

    let commit_msg_arg = commit_msg_path
        .to_str()
        .expect("commit-msg path is valid UTF-8 (worktree path + ASCII filename)");
    let result = run_cmd(
        &["git", "commit", "--allow-empty", "-F", commit_msg_arg],
        wt_path,
        "commit",
        None,
    );
    // Always clean up the commit message file
    let _ = std::fs::remove_file(&commit_msg_path);
    result?;

    run_cmd(
        &["git", "push", "-u", "origin", branch],
        wt_path,
        "push",
        Some(Duration::from_secs(60)),
    )?;

    let pr_body = format!("## What\n\n{}.", prompt);
    let (stdout, _) = run_cmd(
        &[
            "gh",
            "pr",
            "create",
            "--title",
            feature_title,
            "--body",
            &pr_body,
            "--base",
            base_branch,
        ],
        wt_path,
        "pr_create",
        Some(Duration::from_secs(60)),
    )?;

    let pr_url = stdout.trim().to_string();
    let pr_number = extract_pr_number(&pr_url);
    Ok((pr_url, pr_number))
}

/// Testable core with injected root and cwd. Production callers
/// binds them to [`project_root`] and `current_dir()`. Tests supply
/// a `TempDir` for both. Returns a `Value` directly — every error
/// scenario surfaces as a `status: "error"` payload with exit code 0
/// via [`run_impl_main`]. No path returns `Err` at the Rust level.
fn run_impl_with_paths(args: &Args, root: &Path, cwd: &Path) -> Value {
    let branch = &args.branch;
    let feature_title = derive_feature(branch);

    // Update TUI step counter. `args.branch` is clap-supplied —
    // external input. Pattern-match and surface a structured error
    // per `.claude/rules/external-input-validation.md` "CLI
    // subcommand entry callsite discipline".
    let state_path = match FlowPaths::try_new(root, branch) {
        Some(p) => p.state_file(),
        None => {
            return json!({
                "status": "error",
                "message": format!("Invalid branch name: {:?}", branch),
            });
        }
    };
    update_step(&state_path, 3);

    // Read relative_cwd from the state file (captured by init_state at
    // flow-start). Default to empty (worktree root) when the state file
    // is unreadable, parse fails, or the field is absent.
    let state_value = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let relative_cwd = state_value
        .as_ref()
        .and_then(|v| {
            v.get("relative_cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let queue_dir = queue_path(root);

    // Resolve the integration branch from git (single source of truth).
    // The PR will target this branch as `--base`. Fail closed via JSON
    // error envelope when git cannot resolve it.
    let base_branch = match crate::git::default_branch_in(root) {
        Ok(b) => b,
        Err(msg) => {
            release(&args.branch, &queue_dir);
            return json!({
                "status": "error",
                "step": "resolve_base_branch",
                "message": msg,
            });
        }
    };

    // Helper: release lock and return error
    let release_lock = |feature: &str| {
        release(feature, &queue_dir);
    };

    // Read prompt from file if provided. Release lock on failure.
    let prompt = if let Some(ref pf) = args.prompt_file {
        match std::fs::read_to_string(pf) {
            Ok(content) => {
                let _ = std::fs::remove_file(pf);
                content.trim().to_string()
            }
            Err(e) => {
                release_lock(&args.branch);
                return json!({
                    "status": "error",
                    "step": "prompt_file",
                    "message": format!("Could not read prompt file: {}", e),
                });
            }
        }
    } else {
        args.description.clone()
    };

    // Step 1: Create worktree
    let wt_path = match create_worktree(root, branch) {
        Ok(p) => p,
        Err(e) => {
            let _ = append_log(
                root,
                branch,
                &format!("[Phase 1] start-workspace — worktree failed: {}", e.message),
            );
            release_lock(&args.branch);
            return json!({
                "status": "error",
                "step": e.step,
                "message": e.message,
            });
        }
    };
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-workspace — worktree .worktrees/{} (ok)",
            branch
        ),
    );

    // Step 2: Commit, push, create PR
    let (pr_url, pr_number) =
        match initial_commit_push_pr(&wt_path, branch, &feature_title, &prompt, &base_branch) {
            Ok(r) => r,
            Err(e) => {
                let _ = append_log(
                    root,
                    branch,
                    &format!(
                        "[Phase 1] start-workspace — PR creation failed: {}",
                        e.message
                    ),
                );
                release_lock(&args.branch);
                return json!({
                    "status": "error",
                    "step": e.step,
                    "message": e.message,
                });
            }
        };
    let _ = append_log(
        root,
        branch,
        "[Phase 1] start-workspace — commit + push + PR create (ok)",
    );

    // Step 3: Backfill state file
    let repo = detect_repo(Some(cwd));
    let pr_url_clone = pr_url.clone();
    let prompt_clone = prompt.clone();
    let repo_clone = repo.clone();

    if state_path.exists() {
        match mutate_state(&state_path, &mut |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            state["pr_number"] = json!(pr_number);
            state["pr_url"] = json!(&pr_url_clone);
            state["repo"] = match &repo_clone {
                Some(r) => json!(r),
                None => json!(null),
            };
            state["prompt"] = json!(&prompt_clone);
        }) {
            Ok(_) => {}
            Err(e) => {
                let _ = append_log(
                    root,
                    branch,
                    &format!("[Phase 1] start-workspace — backfill failed: {}", e),
                );
                release_lock(&args.branch);
                return json!({
                    "status": "error",
                    "step": "backfill",
                    "message": format!("Failed to backfill state: {}", e),
                });
            }
        }
        let _ = append_log(
            root,
            branch,
            "[Phase 1] start-workspace — state backfill (ok)",
        );
    }

    // Step 4: Apply Flow In-Progress label (best-effort).
    //
    // Runs AFTER worktree, PR, and state backfill all succeed and
    // BEFORE the final lock release. Failure paths above return
    // early without reaching this block, so a failed start-workspace
    // leaves no sticky label that blocks the next retry. Label
    // failures (gh auth, network) do not fail the flow — the label
    // is a coordination signal, not a correctness gate; start_init's
    // pre-lock guard catches cross-machine WIP from the receiving
    // side regardless of whether the label apply here succeeded.
    let issue_numbers = extract_issue_numbers(&prompt);
    if !issue_numbers.is_empty() {
        let result = label_issues(&issue_numbers, "add");
        let _ = append_log(
            root,
            branch,
            &format!(
                "[Phase 1] start-workspace — label-issues (labeled: {:?}, failed: {:?})",
                result.labeled, result.failed
            ),
        );
    }

    // Step 5: Release lock (final action)
    release_lock(&args.branch);
    let _ = append_log(
        root,
        branch,
        "[Phase 1] start-workspace — lock released (ok)",
    );

    // Advance TUI display to step 4 ("entering worktree") before returning
    update_step(&state_path, 4);

    let wt_relative = format!(".worktrees/{}", branch);
    // worktree_cwd is the absolute directory the agent should cd into.
    // For root-level flows it points at the worktree itself; for flows
    // started inside a mono-repo subdirectory (relative_cwd non-empty),
    // it includes that suffix so the agent lands in the same subdir
    // it started from.
    //
    // Absolute, NOT relative — the skill's Step 3 substitutes this
    // value directly into a `cd <worktree_cwd>` command, and the bash
    // tool's cwd at that moment is whatever the user invoked the flow
    // from (project root for a flat repo, or `synapse/`/`cortex/`/etc.
    // for a mono-repo subdir flow). A relative path resolves against
    // bash's current cwd and breaks for any cwd != project_root; an
    // absolute path works from any cwd.
    let wt_abs = root.join(".worktrees").join(branch);
    let worktree_cwd_path = if relative_cwd.is_empty() {
        wt_abs
    } else {
        wt_abs.join(&relative_cwd)
    };
    let worktree_cwd = worktree_cwd_path.to_string_lossy().into_owned();
    json!({
        "status": "ok",
        "worktree": wt_relative,
        "worktree_cwd": worktree_cwd,
        "relative_cwd": relative_cwd,
        "pr_url": pr_url,
        "pr_number": pr_number,
        "feature": feature_title,
        "branch": branch,
    })
}

/// Main-arm entry point: returns the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Takes `root: &Path` and
/// `cwd: &Path` per `.claude/rules/rust-patterns.md` "Main-arm
/// dispatch" so inline tests can pass a `TempDir` fixture instead of
/// the host `project_root()`/`current_dir()`. `run_impl_with_paths`
/// always returns `Value` — business errors appear in the
/// `status: "error"` payload with exit code `0`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    (run_impl_with_paths(args, root, cwd), 0)
}

#[cfg(any())]
mod _removed {
    use super::*;

    #[test]
    fn extract_pr_number_standard_url() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/123"),
            123
        );
    }

    #[test]
    fn extract_pr_number_trailing_slash() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/42/"),
            42
        );
    }

    #[test]
    fn extract_pr_number_malformed() {
        assert_eq!(extract_pr_number("not-a-url"), 0);
    }

    #[test]
    fn extract_pr_number_non_numeric() {
        assert_eq!(extract_pr_number("https://github.com/org/repo/pull/abc"), 0);
    }

    #[test]
    fn extract_pr_number_empty_string() {
        assert_eq!(extract_pr_number(""), 0);
    }

    #[test]
    fn extract_pr_number_pull_with_no_number() {
        // URL ends at "pull/" with nothing parseable after it
        assert_eq!(extract_pr_number("https://github.com/org/repo/pull/"), 0);
    }

    // --- run_impl_main ---

    /// Drives run_impl_main against a bare TempDir that is not a git
    /// repo — the worktree-creation subprocess fails on missing
    /// `.git`, and `run_impl_with_paths` returns a `status:"error"`
    /// `step:"worktree"` payload. run_impl_main wraps with exit 0
    /// per the business-error convention.
    #[test]
    fn start_workspace_run_impl_main_err_path() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Seed just enough state so the function reaches the
        // worktree-creation step. No .git, so create_worktree fails.
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let args = Args {
            description: "workspace-err-feature".to_string(),
            branch: "workspace-err-branch".to_string(),
            prompt_file: None,
        };
        let (v, code) = run_impl_main(&args, &root, &root);
        assert_eq!(code, 0, "exit code is 0 for business errors");
        assert_eq!(v["status"], "error");
    }
}
