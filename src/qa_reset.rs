//! Reset a QA repo to seed state.
//!
//! Usage: bin/flow qa-reset --repo <owner/repo> [--local-path <path>]
//!
//! Resets git to the seed tag, closes PRs, deletes remote branches,
//! recreates issues from .qa/issues.json template.

use std::path::Path;
use std::process::{Command, Stdio};

use clap::Parser;
use serde_json::{json, Value};

/// Result of a subprocess invocation for the injectable runner.
/// Shared with scaffold_qa via pub import — do not modify signature
/// without checking all callers.
pub struct CmdResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Production subprocess runner. Captures stdout/stderr and returns
/// a CmdResult. Shared by qa_reset and scaffold_qa run_impl functions.
pub fn default_runner(cmd_args: &[&str], cwd: Option<&Path>) -> CmdResult {
    let mut command = Command::new(cmd_args[0]);
    command.args(&cmd_args[1..]);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    match command.output() {
        Ok(output) => CmdResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Err(e) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

#[derive(Parser, Debug)]
#[command(name = "qa-reset", about = "Reset a QA repo to seed state")]
pub struct Args {
    /// GitHub repo (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Local clone path
    #[arg(long)]
    pub local_path: Option<String>,
}

/// Reset local repo to seed tag and force push.
pub fn reset_git(local_path: &Path, runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult) -> Value {
    let commands: Vec<Vec<&str>> = vec![
        vec!["git", "reset", "--hard", "seed"],
        vec!["git", "push", "-f", "origin", "main"],
    ];
    for cmd in &commands {
        let result = runner(cmd, Some(local_path));
        if !result.success {
            return json!({
                "status": "error",
                "message": format!("{} failed: {}", cmd[..3].join(" "), result.stderr.trim())
            });
        }
    }
    json!({"status": "ok"})
}

/// Close all open PRs in the repo. Returns count closed.
pub fn close_prs(repo: &str, runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult) -> usize {
    let result = runner(
        &[
            "gh", "pr", "list", "--repo", repo, "--state", "open", "--json", "number",
        ],
        None,
    );
    if !result.success {
        return 0;
    }

    let prs: Vec<Value> = serde_json::from_str(result.stdout.trim()).unwrap_or_default();
    let mut closed = 0;
    for pr in &prs {
        if let Some(num) = pr["number"].as_i64() {
            let num_str = num.to_string();
            let r = runner(&["gh", "pr", "close", &num_str, "--repo", repo], None);
            if r.success {
                closed += 1;
            }
        }
    }
    closed
}

/// Delete all remote branches except main. Returns count deleted.
pub fn delete_remote_branches(
    repo: &str,
    local_path: &Path,
    runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult,
) -> usize {
    let _ = repo; // repo unused — branches deleted via git push from local_path
    let result = runner(&["git", "branch", "-r"], Some(local_path));
    if !result.success {
        return 0;
    }

    let mut branches = Vec::new();
    for line in result.stdout.trim().split('\n') {
        let branch = line.trim();
        if branch.is_empty() {
            continue;
        }
        let remote_name = if let Some(pos) = branch.find('/') {
            &branch[pos + 1..]
        } else {
            branch
        };
        if remote_name == "main" || remote_name == "HEAD -> origin/main" {
            continue;
        }
        branches.push(remote_name.to_string());
    }

    let mut deleted = 0;
    for branch in &branches {
        let r = runner(
            &["git", "push", "origin", "--delete", branch],
            Some(local_path),
        );
        if r.success {
            deleted += 1;
        }
    }
    deleted
}

/// Load the .qa/issues.json template from the repo via GitHub API.
pub fn load_issue_template(
    repo: &str,
    runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult,
) -> Vec<Value> {
    let api_path = format!("repos/{}/contents/.qa/issues.json", repo);
    let result = runner(&["gh", "api", &api_path, "--jq", ".content"], None);
    if !result.success {
        return Vec::new();
    }

    let decoded = base64_decode(result.stdout.trim());
    match decoded {
        Some(content) => serde_json::from_str(&content).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Decode a standard base64 string (with optional embedded whitespace
/// from the GitHub API content envelope). Inlined here so qa-reset
/// stays consistent with FLOW's zero-dependency philosophy and does
/// not pull in a base64 crate just for one call site.
fn base64_decode(input: &str) -> Option<String> {
    // Strip whitespace that GitHub API may include
    let clean: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    // Simple base64 decoder
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &b) in alphabet.iter().enumerate() {
        lookup[b as usize] = i as u8;
    }

    let bytes = clean.as_bytes();
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        if b == b'=' {
            break;
        }
        let val = lookup[b as usize];
        if val == 255 {
            return None; // Invalid character
        }
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    String::from_utf8(output).ok()
}

/// Close all existing issues and recreate from template. Returns count created.
pub fn reset_issues(
    repo: &str,
    template: &[Value],
    runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult,
) -> usize {
    // Close existing issues
    let result = runner(
        &[
            "gh", "issue", "list", "--repo", repo, "--state", "all", "--json", "number",
        ],
        None,
    );
    if result.success && !result.stdout.trim().is_empty() {
        if let Ok(issues) = serde_json::from_str::<Vec<Value>>(result.stdout.trim()) {
            for issue in &issues {
                if let Some(num) = issue["number"].as_i64() {
                    let num_str = num.to_string();
                    runner(&["gh", "issue", "close", &num_str, "--repo", repo], None);
                }
            }
        }
    }

    // Create new issues from template
    let mut created = 0;
    for issue in template {
        let title = issue["title"].as_str().unwrap_or("");
        let body = issue["body"].as_str().unwrap_or("");
        let labels: Vec<&str> = issue["labels"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut cmd: Vec<&str> = vec![
            "gh", "issue", "create", "--repo", repo, "--title", title, "--body", body,
        ];
        for label in &labels {
            cmd.push("--label");
            cmd.push(label);
        }

        let r = runner(&cmd, None);
        if r.success {
            created += 1;
        }
    }
    created
}

/// Remove FLOW artifacts from a local clone.
pub fn clean_local(local_path: &Path) {
    for name in &[".flow-states", ".claude"] {
        let target = local_path.join(name);
        if target.is_dir() {
            let _ = std::fs::remove_dir_all(&target);
        }
    }
    let flow_json = local_path.join(".flow.json");
    if flow_json.exists() {
        let _ = std::fs::remove_file(&flow_json);
    }
}

/// Full reset workflow.
pub fn reset_impl(
    repo: &str,
    local_path: Option<&str>,
    runner: &dyn Fn(&[&str], Option<&Path>) -> CmdResult,
) -> Value {
    if let Some(lp) = local_path {
        let path = Path::new(lp);
        let git_result = reset_git(path, runner);
        if git_result["status"] != "ok" {
            return git_result;
        }
    }

    let prs_closed = close_prs(repo, runner);
    let branches_deleted = if let Some(lp) = local_path {
        delete_remote_branches(repo, Path::new(lp), runner)
    } else {
        0
    };

    let template = load_issue_template(repo, runner);
    let issues_reset = reset_issues(repo, &template, runner);

    if let Some(lp) = local_path {
        clean_local(Path::new(lp));
    }

    json!({
        "status": "ok",
        "prs_closed": prs_closed,
        "branches_deleted": branches_deleted,
        "issues_reset": issues_reset
    })
}

/// CLI entry point.
///
/// Returns Ok(Value) for both success and status-error responses.
/// Returns Err(String) only for infrastructure failures.
/// The run() wrapper prints the result and exits 1 on status-error
/// so failed reset attempts surface as a non-zero exit to the calling
/// QA skill, while successful resets always exit 0.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    Ok(reset_impl(
        &args.repo,
        args.local_path.as_deref(),
        &default_runner,
    ))
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    fn ok_result(stdout: &str) -> CmdResult {
        CmdResult {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn err_result(stderr: &str) -> CmdResult {
        CmdResult {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    // --- reset_git ---

    #[test]
    fn test_reset_git_runs_correct_commands() {
        let cmds = RefCell::new(Vec::new());
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            cmds.borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            ok_result("")
        };

        let result = reset_git(Path::new("/tmp/repo"), &runner);
        assert_eq!(result["status"], "ok");

        let captured = cmds.borrow();
        assert!(captured.iter().any(|c| c.contains(&"reset".to_string())));
        assert!(captured.iter().any(|c| c.contains(&"push".to_string())));
    }

    #[test]
    fn test_reset_git_failure() {
        let runner =
            |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("fatal: not a repo") };

        let result = reset_git(Path::new("/tmp/repo"), &runner);
        assert_eq!(result["status"], "error");
    }

    // --- close_prs ---

    #[test]
    fn test_close_prs_closes_all_open() {
        let pr_list = serde_json::to_string(&json!([{"number": 1}, {"number": 2}])).unwrap();
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"list") {
                ok_result(&pr_list)
            } else {
                ok_result("")
            }
        };

        let result = close_prs("owner/repo", &runner);
        assert_eq!(result, 2);
    }

    #[test]
    fn test_close_prs_no_open() {
        let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("[]") };

        let result = close_prs("owner/repo", &runner);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_close_prs_gh_failure() {
        let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("error") };

        let result = close_prs("owner/repo", &runner);
        assert_eq!(result, 0);
    }

    // --- delete_remote_branches ---

    #[test]
    fn test_delete_remote_branches() {
        let branch_output = "  origin/main\n  origin/feature-1\n  origin/feature-2\n";
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"-r") {
                ok_result(branch_output)
            } else {
                ok_result("")
            }
        };

        let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
        assert_eq!(result, 2);
    }

    #[test]
    fn test_delete_remote_branches_only_main() {
        let runner =
            |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("  origin/main\n") };

        let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_delete_remote_branches_git_failure() {
        let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("error") };

        let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_delete_remote_branches_empty_line() {
        let branch_output = "  origin/main\n\n  origin/feature-1\n";
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"-r") {
                ok_result(branch_output)
            } else {
                ok_result("")
            }
        };

        let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
        assert_eq!(result, 1);
    }

    // --- load_issue_template ---

    #[test]
    fn test_load_issue_template_success() {
        let content =
            serde_json::to_string(&json!([{"title": "Test", "body": "Body", "labels": []}]))
                .unwrap();
        let encoded = simple_base64_encode(content.as_bytes());
        let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result(&encoded) };

        let result = load_issue_template("owner/repo", &runner);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["title"], "Test");
    }

    #[test]
    fn test_load_issue_template_failure() {
        let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("not found") };

        let result = load_issue_template("owner/repo", &runner);
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_issue_template_corrupt() {
        let runner =
            |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("not-base64!!!") };

        let result = load_issue_template("owner/repo", &runner);
        assert!(result.is_empty());
    }

    // --- reset_issues ---

    #[test]
    fn test_reset_issues_closes_and_recreates() {
        let issue_list = serde_json::to_string(&json!([{"number": 1}, {"number": 2}])).unwrap();
        let close_count = RefCell::new(0usize);
        let create_count = RefCell::new(0usize);

        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"list") {
                ok_result(&issue_list)
            } else if args.contains(&"close") {
                *close_count.borrow_mut() += 1;
                ok_result("")
            } else if args.contains(&"create") {
                *create_count.borrow_mut() += 1;
                ok_result("")
            } else {
                ok_result("")
            }
        };

        let template = vec![json!({"title": "New issue", "body": "Body", "labels": []})];
        let result = reset_issues("owner/repo", &template, &runner);

        assert_eq!(result, 1);
        assert_eq!(*close_count.borrow(), 2);
        assert_eq!(*create_count.borrow(), 1);
    }

    #[test]
    fn test_reset_issues_with_labels() {
        let calls = RefCell::new(Vec::new());
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if args.contains(&"list") {
                ok_result("[]")
            } else {
                ok_result("")
            }
        };

        let template = vec![json!({"title": "Bug", "body": "Fix it", "labels": ["bug", "urgent"]})];
        let result = reset_issues("owner/repo", &template, &runner);

        assert_eq!(result, 1);
        let captured = calls.borrow();
        let create_call = captured
            .iter()
            .find(|c| c.contains(&"create".to_string()))
            .unwrap();
        assert!(create_call.contains(&"--label".to_string()));
        assert!(create_call.contains(&"bug".to_string()));
        assert!(create_call.contains(&"urgent".to_string()));
    }

    // --- clean_local ---

    #[test]
    fn test_clean_local_removes_flow_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".flow-states")).unwrap();
        fs::write(dir.path().join(".flow-states").join("test.json"), "{}").unwrap();
        fs::write(dir.path().join(".flow.json"), "{}").unwrap();
        fs::create_dir(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude").join("settings.json"), "{}").unwrap();

        clean_local(dir.path());

        assert!(!dir.path().join(".flow-states").exists());
        assert!(!dir.path().join(".flow.json").exists());
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn test_clean_local_missing_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        // No artifacts exist — should not panic
        clean_local(dir.path());
    }

    // --- reset_impl ---

    #[test]
    fn test_reset_full_workflow() {
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"list") && args.contains(&"--state") && args.contains(&"open") {
                // PR list
                ok_result(&serde_json::to_string(&json!([{"number": 1}, {"number": 2}])).unwrap())
            } else if args.contains(&"list") && args.contains(&"--state") && args.contains(&"all") {
                // Issue list
                ok_result("[]")
            } else if args.contains(&"-r") {
                // branch -r
                ok_result(
                    "  origin/main\n  origin/feature-1\n  origin/feature-2\n  origin/feature-3\n",
                )
            } else if args.contains(&"api") {
                // load_issue_template
                let content =
                    serde_json::to_string(&json!([{"title": "T", "body": "B", "labels": []}]))
                        .unwrap();
                ok_result(&simple_base64_encode(content.as_bytes()))
            } else {
                ok_result("")
            }
        };

        let dir = tempfile::tempdir().unwrap();
        let result = reset_impl("owner/repo", Some(dir.path().to_str().unwrap()), &runner);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["prs_closed"], 2);
        assert_eq!(result["branches_deleted"], 3);
        assert_eq!(result["issues_reset"], 1);
    }

    #[test]
    fn test_reset_without_local_path() {
        let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
            if args.contains(&"list") {
                ok_result("[]")
            } else {
                ok_result("")
            }
        };

        let result = reset_impl("owner/repo", None, &runner);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["branches_deleted"], 0);
    }

    #[test]
    fn test_reset_git_failure_stops_early() {
        let runner =
            |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("not a repo") };

        let result = reset_impl("owner/repo", Some("/tmp/repo"), &runner);
        assert_eq!(result["status"], "error");
    }

    /// Simple base64 encoder for test use only.
    fn simple_base64_encode(input: &[u8]) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let n = (b0 << 16) | (b1 << 8) | b2;
            result.push(alphabet[(n >> 18 & 63) as usize] as char);
            result.push(alphabet[(n >> 12 & 63) as usize] as char);
            if chunk.len() > 1 {
                result.push(alphabet[(n >> 6 & 63) as usize] as char);
            } else {
                result.push('=');
            }
            if chunk.len() > 2 {
                result.push(alphabet[(n & 63) as usize] as char);
            } else {
                result.push('=');
            }
        }
        result
    }
}
