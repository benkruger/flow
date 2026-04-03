//! Port of lib/start-setup.py — consolidated setup for FLOW Start phase.
//!
//! Creates worktree, makes initial commit + push + PR, creates/backfills
//! state file, and logs all operations.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::github::detect_repo;
use crate::lock::mutate_state;
use crate::phase_config::{auto_skills, build_initial_phases, freeze_phases, read_flow_json};
use crate::state::SkillConfig;
use crate::utils::{branch_name, derive_feature, detect_tty, extract_issue_numbers, now, read_prompt_file};

#[derive(Parser, Debug)]
#[command(name = "start-setup", about = "FLOW Start phase setup")]
pub struct Args {
    /// Feature name words
    pub feature_name: Option<String>,

    /// Full start prompt (preserved verbatim in state file)
    #[arg(long)]
    pub prompt: Option<String>,

    /// Path to file containing start prompt (file is deleted after reading)
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,

    /// Skip git pull (caller already pulled main)
    #[arg(long = "skip-pull")]
    pub skip_pull: bool,

    /// Override all skills to fully autonomous preset
    #[arg(long)]
    pub auto: bool,
}

/// Error during setup with step identification.
#[derive(Debug)]
pub struct SetupError {
    pub step: String,
    pub message: String,
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.step, self.message)
    }
}

/// Info about a duplicate flow targeting the same issue.
#[derive(Debug)]
pub struct DuplicateInfo {
    pub branch: String,
    pub phase: String,
    pub pr_url: String,
}

/// Run a shell command, returning (stdout, stderr). Returns Err on failure.
pub fn run_cmd(
    args: &[&str],
    cwd: &Path,
    step_name: &str,
    timeout: Option<Duration>,
) -> Result<(String, String), SetupError> {
    let mut child = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| SetupError {
            step: step_name.to_string(),
            message: format!("Failed to spawn: {}", e),
        })?;

    if let Some(dur) = timeout {
        match child.wait_timeout(dur) {
            Ok(Some(status)) => {
                let output = child.wait_with_output().map_err(|e| SetupError {
                    step: step_name.to_string(),
                    message: e.to_string(),
                })?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return Err(SetupError {
                        step: step_name.to_string(),
                        message: if stderr.is_empty() { stdout } else { stderr },
                    });
                }
                Ok((
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(SetupError {
                    step: step_name.to_string(),
                    message: format!("Timed out after {}s", dur.as_secs()),
                })
            }
            Err(e) => Err(SetupError {
                step: step_name.to_string(),
                message: e.to_string(),
            }),
        }
    } else {
        let output = child.wait_with_output().map_err(|e| SetupError {
            step: step_name.to_string(),
            message: e.to_string(),
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(SetupError {
                step: step_name.to_string(),
                message: if stderr.is_empty() { stdout } else { stderr },
            });
        }
        Ok((
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Extract PR number from URL like https://github.com/org/repo/pull/123.
pub fn extract_pr_number(pr_url: &str) -> u32 {
    let parts: Vec<&str> = pr_url.trim_end_matches('/').split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "pull" && i + 1 < parts.len() {
            if let Ok(n) = parts[i + 1].parse::<u32>() {
                return n;
            }
        }
    }
    0
}

/// Fetch issue title from GitHub. Returns title string or None on failure.
/// Uses a 10-second timeout matching the Python implementation.
pub fn fetch_issue_title(issue_number: i64) -> Option<String> {
    let dir = std::env::current_dir().ok()?;
    let (stdout, _) = run_cmd(
        &[
            "gh",
            "issue",
            "view",
            &issue_number.to_string(),
            "--json",
            "title",
            "--jq",
            ".title",
        ],
        &dir,
        "fetch_issue_title",
        Some(Duration::from_secs(10)),
    )
    .ok()?;

    let title = stdout.trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// Check if an existing flow already targets the same issue numbers.
pub fn check_duplicate_issue(
    project_root: &Path,
    issue_numbers: &[i64],
    self_branch: &str,
) -> Option<DuplicateInfo> {
    if issue_numbers.is_empty() {
        return None;
    }
    let state_dir = project_root.join(".flow-states");
    if !state_dir.is_dir() {
        return None;
    }
    let target_issues: std::collections::HashSet<i64> = issue_numbers.iter().copied().collect();

    let mut entries: Vec<_> = std::fs::read_dir(&state_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.ends_with(".json") {
            continue;
        }
        if name_str.ends_with("-phases.json") {
            continue;
        }
        let stem = name_str.trim_end_matches(".json");
        if stem == self_branch {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let state: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let prompt = state
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let existing_issues: std::collections::HashSet<i64> =
            extract_issue_numbers(prompt).into_iter().collect();

        if !existing_issues.is_disjoint(&target_issues) {
            return Some(DuplicateInfo {
                branch: state
                    .get("branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or(stem)
                    .to_string(),
                phase: state
                    .get("current_phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                pr_url: state
                    .get("pr_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    None
}

/// Pull latest main.
pub fn git_pull(cwd: &Path) -> Result<(), SetupError> {
    run_cmd(&["git", "pull", "origin", "main"], cwd, "git_pull", Some(Duration::from_secs(60)))?;
    Ok(())
}

/// Create a git worktree at .worktrees/<branch>.
pub fn create_worktree(project_root: &Path, branch: &str) -> Result<PathBuf, SetupError> {
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

    // Symlink .venv if it exists
    let venv_dir = project_root.join(".venv");
    if venv_dir.is_dir() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink(
                std::path::Path::new("../..").join(".venv"),
                wt_path.join(".venv"),
            );
        }
    }

    Ok(wt_path)
}

/// Make empty commit, push, and create PR. Returns (pr_url, pr_number).
pub fn initial_commit_push_pr(
    wt_path: &Path,
    branch: &str,
    feature_title: &str,
    prompt: &str,
) -> Result<(String, u32), SetupError> {
    let commit_msg_path = wt_path.join(".flow-commit-msg");
    std::fs::write(&commit_msg_path, format!("Start {} branch", branch)).map_err(|e| {
        SetupError {
            step: "commit".to_string(),
            message: e.to_string(),
        }
    })?;

    let result = run_cmd(
        &["git", "commit", "--allow-empty", "-F", ".flow-commit-msg"],
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
            "main",
        ],
        wt_path,
        "pr_create",
        Some(Duration::from_secs(60)),
    )?;

    let pr_url = stdout.trim().to_string();
    let pr_number = extract_pr_number(&pr_url);
    Ok((pr_url, pr_number))
}

/// Create the FLOW state file (fallback when init-state didn't create one).
pub fn create_state_file(
    project_root: &Path,
    branch: &str,
    pr_url: &str,
    pr_number: u32,
    framework: &str,
    skills: Option<&IndexMap<String, SkillConfig>>,
    prompt: &str,
    repo: Option<&str>,
) -> Result<Value, SetupError> {
    let current_time = now();
    let phases = build_initial_phases(&current_time);

    // Serialize phases to serde_json::Value preserving order
    let phases_value: Value = serde_json::to_value(&phases).map_err(|e| SetupError {
        step: "state".to_string(),
        message: e.to_string(),
    })?;

    let mut state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": repo,
        "pr_number": pr_number,
        "pr_url": pr_url,
        "started_at": current_time,
        "current_phase": "flow-start",
        "framework": framework,
        "files": {
            "plan": null,
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch),
        },
        "session_tty": detect_tty(),
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": prompt,
        "phases": phases_value,
        "phase_transitions": [],
    });

    if let Some(sk) = skills {
        let sk_value: Value = serde_json::to_value(sk).map_err(|e| SetupError {
            step: "state".to_string(),
            message: e.to_string(),
        })?;
        state["skills"] = sk_value;
    }

    let state_dir = project_root.join(".flow-states");
    std::fs::create_dir_all(&state_dir).map_err(|e| SetupError {
        step: "state".to_string(),
        message: e.to_string(),
    })?;
    let state_path = state_dir.join(format!("{}.json", branch));
    std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .map_err(|e| SetupError {
        step: "state".to_string(),
        message: e.to_string(),
    })?;

    Ok(state)
}

/// Main entry point for start-setup.
pub fn run(args: Args) {
    let feature_name = match &args.feature_name {
        Some(f) if !f.is_empty() => f.clone(),
        _ => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": "args",
                    "message": "Feature name required. Usage: flow-rs start-setup \"<feature name>\""
                })
            );
            std::process::exit(1);
        }
    };

    // Resolve prompt
    let raw_prompt = if let Some(ref pf) = args.prompt_file {
        match read_prompt_file(Path::new(pf)) {
            Ok(content) => content,
            Err(e) => {
                println!(
                    "{}",
                    json!({
                        "status": "error",
                        "step": "prompt_file",
                        "message": e.to_string(),
                    })
                );
                std::process::exit(1);
            }
        }
    } else if let Some(ref p) = args.prompt {
        p.clone()
    } else {
        feature_name.clone()
    };

    // Issue-aware branch naming
    let issue_numbers = extract_issue_numbers(&raw_prompt);
    let naming_words = if !issue_numbers.is_empty() {
        match fetch_issue_title(issue_numbers[0]) {
            Some(title) => title,
            None => feature_name.clone(),
        }
    } else {
        feature_name.clone()
    };

    let branch = branch_name(&naming_words);
    let feature_title = derive_feature(&branch);
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Duplicate issue guard
    if !issue_numbers.is_empty() {
        let self_branch_name = branch_name(&feature_name);
        if let Some(dup) = check_duplicate_issue(&project_root, &issue_numbers, &self_branch_name) {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": "duplicate_issue",
                    "message": format!(
                        "Issue already has an active flow on branch '{}' (phase: {}, PR: {}). Resume the existing flow instead.",
                        dup.branch, dup.phase, dup.pr_url
                    ),
                })
            );
            std::process::exit(1);
        }
    }

    // Read framework from .flow.json
    let init_data = match read_flow_json(Some(&project_root)) {
        Some(d) => d,
        None => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": "flow_json",
                    "message": "Could not read .flow.json"
                })
            );
            std::process::exit(1);
        }
    };
    let framework = init_data
        .get("framework")
        .and_then(|v| v.as_str())
        .unwrap_or("rails")
        .to_string();

    let skills: Option<IndexMap<String, SkillConfig>> = if args.auto {
        Some(auto_skills())
    } else {
        init_data.get("skills").and_then(|v| {
            serde_json::from_value::<IndexMap<String, SkillConfig>>(v.clone()).ok()
        })
    };

    // Git pull (skip when caller already pulled main)
    if !args.skip_pull {
        if let Err(e) = git_pull(&project_root) {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": e.step,
                    "message": e.message,
                })
            );
            return;
        }
        let _ = append_log(&project_root, &branch, "[Phase 1] git pull origin main (exit 0)");
    }

    // Create worktree
    let wt_path = match create_worktree(&project_root, &branch) {
        Ok(p) => p,
        Err(e) => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": e.step,
                    "message": e.message,
                })
            );
            return;
        }
    };
    let _ = append_log(
        &project_root,
        &branch,
        &format!("[Phase 1] git worktree add .worktrees/{} (exit 0)", branch),
    );

    // Commit, push, PR
    let (pr_url, pr_number) = match initial_commit_push_pr(&wt_path, &branch, &feature_title, &raw_prompt) {
        Ok(r) => r,
        Err(e) => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "step": e.step,
                    "message": e.message,
                })
            );
            return;
        }
    };
    let _ = append_log(
        &project_root,
        &branch,
        "[Phase 1] git commit + push + gh pr create (exit 0)",
    );

    // Detect repo
    let repo = detect_repo(Some(project_root.as_path()));

    // Update or create state file
    let state_path = project_root
        .join(".flow-states")
        .join(format!("{}.json", branch));

    if state_path.exists() {
        // Backfill PR fields and prompt into existing state file
        let pr_url_clone = pr_url.clone();
        let prompt_clone = raw_prompt.clone();
        let repo_clone = repo.clone();
        match mutate_state(&state_path, move |state| {
            state["pr_number"] = json!(pr_number);
            state["pr_url"] = json!(pr_url_clone);
            state["repo"] = match &repo_clone {
                Some(r) => json!(r),
                None => json!(null),
            };
            state["prompt"] = json!(prompt_clone);
        }) {
            Ok(_) => {}
            Err(e) => {
                println!(
                    "{}",
                    json!({
                        "status": "error",
                        "step": "backfill",
                        "message": format!("Failed to backfill state: {}", e),
                    })
                );
                return;
            }
        }
        let _ = append_log(
            &project_root,
            &branch,
            &format!("[Phase 1] backfill .flow-states/{}.json (exit 0)", branch),
        );
    } else {
        // Create state file from scratch
        match create_state_file(
            &project_root,
            &branch,
            &pr_url,
            pr_number,
            &framework,
            skills.as_ref(),
            &raw_prompt,
            repo.as_deref(),
        ) {
            Ok(_) => {}
            Err(e) => {
                println!(
                    "{}",
                    json!({
                        "status": "error",
                        "step": e.step,
                        "message": e.message,
                    })
                );
                return;
            }
        }
        let _ = append_log(
            &project_root,
            &branch,
            &format!("[Phase 1] create .flow-states/{}.json (exit 0)", branch),
        );

        // Freeze phase config
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let phases_json = manifest_dir.join("flow-phases.json");
        if phases_json.exists() {
            let _ = freeze_phases(&phases_json, &project_root, &branch);
            let _ = append_log(
                &project_root,
                &branch,
                &format!(
                    "[Phase 1] freeze .flow-states/{}-phases.json (exit 0)",
                    branch
                ),
            );
        }
    }

    println!(
        "{}",
        json!({
            "status": "ok",
            "worktree": format!(".worktrees/{}", branch),
            "pr_url": pr_url,
            "pr_number": pr_number,
            "feature": feature_title,
            "branch": branch,
        })
    );
}

// --- wait_timeout helper for child processes ---
trait WaitTimeout {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::thread;

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match self.try_wait()? {
                Some(status) => {
                    return Ok(Some(status));
                }
                None => {
                    if start.elapsed() >= dur {
                        return Ok(None);
                    }
                    thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/abc"),
            0
        );
    }

    #[test]
    fn check_duplicate_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_duplicate_issue(dir.path(), &[] as &[i64], "any").is_none());
    }

    #[test]
    fn check_duplicate_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "any").is_none());
    }

    #[test]
    fn check_duplicate_detects_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            json!({
                "prompt": "work on issue #123",
                "branch": "existing-branch",
                "current_phase": "flow-code",
                "pr_url": "https://github.com/test/repo/pull/99",
            })
            .to_string(),
        )
        .unwrap();
        let result = check_duplicate_issue(dir.path(), &[123], "new-branch");
        assert!(result.is_some());
        let dup = result.unwrap();
        assert_eq!(dup.branch, "existing-branch");
        assert_eq!(dup.phase, "flow-code");
        assert_eq!(dup.pr_url, "https://github.com/test/repo/pull/99");
    }

    #[test]
    fn check_duplicate_no_false_positive() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            json!({
                "prompt": "work on issue #123",
                "branch": "existing-branch",
                "current_phase": "flow-code",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[456], "new-branch").is_none());
    }

    #[test]
    fn check_duplicate_multi_issue_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("existing-branch.json"),
            json!({
                "prompt": "work on issue #456",
                "branch": "existing-branch",
                "current_phase": "flow-plan",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        let result = check_duplicate_issue(dir.path(), &[123, 456], "new-branch");
        assert!(result.is_some());
    }

    #[test]
    fn check_duplicate_skips_self_branch() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("my-branch.json"),
            json!({
                "prompt": "work on issue #123",
                "branch": "my-branch",
                "current_phase": "flow-start",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "my-branch").is_none());
    }

    #[test]
    fn check_duplicate_skips_phases_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("some-branch-phases.json"),
            json!({
                "prompt": "work on issue #123",
                "branch": "some-branch",
                "current_phase": "flow-code",
                "pr_url": "",
            })
            .to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn check_duplicate_skips_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("bad-json.json"), "not valid json {{{").unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn check_duplicate_null_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("null-prompt.json"),
            json!({"prompt": null, "branch": "null-prompt"}).to_string(),
        )
        .unwrap();
        assert!(check_duplicate_issue(dir.path(), &[123], "other-branch").is_none());
    }

    #[test]
    fn run_cmd_echo_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let (stdout, _) = run_cmd(&["echo", "hello"], dir.path(), "echo_step", None).unwrap();
        assert_eq!(stdout, "hello");
    }

    #[test]
    fn run_cmd_failure_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_cmd(&["false"], dir.path(), "fail_step", None);
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert_eq!(e.step, "fail_step");
    }

    #[test]
    fn run_cmd_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_cmd(
            &["sleep", "10"],
            dir.path(),
            "timeout_step",
            Some(Duration::from_millis(100)),
        );
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert_eq!(e.step, "timeout_step");
        assert!(e.message.contains("Timed out"));
    }
}
