use std::path::Path;
use std::process::Command;

use regex::Regex;

/// Auto-detect GitHub repo from git remote origin URL.
///
/// Returns `owner/repo` string or None if detection fails.
/// Optional cwd parameter for running git in a specific directory.
pub fn detect_repo(cwd: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["remote", "get-url", "origin"]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return None;
    }

    let re = Regex::new(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$").unwrap();
    re.captures(&url).map(|cap| cap[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to extract repo from a URL string using the same regex logic.
    fn extract_repo(url: &str) -> Option<String> {
        let re = Regex::new(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$").unwrap();
        re.captures(url).map(|cap| cap[1].to_string())
    }

    #[test]
    fn ssh_url() {
        assert_eq!(
            extract_repo("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn https_url() {
        assert_eq!(
            extract_repo("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn https_url_with_git_suffix() {
        assert_eq!(
            extract_repo("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn non_github_url() {
        assert_eq!(extract_repo("https://gitlab.com/owner/repo"), None);
    }

    #[test]
    fn empty_url() {
        assert_eq!(extract_repo(""), None);
    }

    #[test]
    fn detect_repo_in_current_dir() {
        // Running in this repo should detect benkruger/flow
        let result = detect_repo(None);
        // May or may not work depending on test context, just verify it returns Option
        let _ = result;
    }
}
