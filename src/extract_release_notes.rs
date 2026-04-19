//! Extract release notes for a version.
//!
//! Reads RELEASE-NOTES.md, finds the section matching the given version,
//! and writes it to `tmp/release-notes-<version>.md`.
//!
//! Usage:
//!   bin/flow extract-release-notes <version>
//!
//! Output (human-readable to stdout):
//!   Success: "Written to <path>"
//!   Error:   "Error: ..." (exit 1)

use std::fs;
use std::path::Path;

use clap::Parser;
use regex::Regex;

use crate::utils::plugin_root;

#[derive(Parser, Debug)]
#[command(
    name = "extract-release-notes",
    about = "Extract release notes for a version"
)]
pub struct Args {
    /// Version to extract (e.g. v0.2.0 or 0.2.0)
    pub version: Option<String>,
}

/// Check whether a `## ` header line contains the version as a complete token.
///
/// After the version substring, the next character must be whitespace, an
/// em-dash, end-of-string, or another non-alphanumeric/non-dot character.
/// This prevents `v0.1.0` from matching a `v0.10.0` header.
fn header_matches_version(line: &str, version: &str) -> bool {
    if let Some(pos) = line.find(version) {
        let after = pos + version.len();
        if after >= line.len() {
            return true;
        }
        let next_char = line.as_bytes()[after];
        // Version token ends at whitespace, dash, or end of line — not
        // at another digit or dot which would mean a longer version.
        !next_char.is_ascii_digit() && next_char != b'.'
    } else {
        false
    }
}

/// Extract the release notes section for a given version from content.
///
/// Finds the `## ` header line where the version appears as a complete
/// token (not a substring of a longer version — e.g. `v0.1.0` does not
/// match a `v0.10.0` header). Collects all lines until the next `## `
/// header and returns the trimmed result. Returns an empty string if
/// the version is not found.
pub fn extract(version: &str, content: &str) -> String {
    let mut section: Vec<&str> = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.starts_with("## ") && header_matches_version(line, version) {
            in_section = true;
            section.push(line);
        } else if line.starts_with("## ") && in_section {
            break;
        } else if in_section {
            section.push(line);
        }
    }

    section.join("\n").trim().to_string()
}

/// Orchestrate extraction and file writing.
///
/// Returns Ok(message) on success, Err(error_text) on failure.
pub fn run_impl(args: &Args, repo_root: &Path) -> Result<String, String> {
    let version = match &args.version {
        Some(v) => v,
        None => return Err("Usage: bin/flow extract-release-notes <version>".to_string()),
    };

    let re = Regex::new(r"^v?\d+\.\d+\.\d+$").unwrap();
    if !re.is_match(version) {
        return Err(format!("Error: invalid version format: {}", version));
    }

    let notes_file = repo_root.join("RELEASE-NOTES.md");
    if !notes_file.exists() {
        return Err(format!("Error: {} not found", notes_file.display()));
    }

    let content = fs::read_to_string(&notes_file)
        .map_err(|e| format!("Error reading {}: {}", notes_file.display(), e))?;

    let extracted = extract(version, &content);
    if extracted.is_empty() {
        return Err(format!("Error: no section found for version {}", version));
    }

    let out_dir = repo_root.join("tmp");
    fs::create_dir_all(&out_dir).map_err(|e| format!("Error creating tmp dir: {}", e))?;

    let out_path = out_dir.join(format!("release-notes-{}.md", version));
    fs::write(&out_path, format!("{}\n", extracted))
        .map_err(|e| format!("Error writing {}: {}", out_path.display(), e))?;

    Ok(format!("Written to {}", out_path.display()))
}

pub fn run(args: Args) {
    let repo_root = match plugin_root() {
        Some(r) => r,
        None => {
            eprintln!("Error: could not find FLOW plugin root");
            std::process::exit(1);
        }
    };

    match run_impl(&args, &repo_root) {
        Ok(output) => {
            println!("{}", output);
        }
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_repo_with_notes(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("RELEASE-NOTES.md"), content).unwrap();
        (dir, root)
    }

    // --- header_matches_version ---

    #[test]
    fn header_matches_exact_version() {
        assert!(header_matches_version("## v0.1.0 — Release", "v0.1.0"));
    }

    #[test]
    fn header_does_not_match_longer_version() {
        assert!(!header_matches_version("## v0.10.0 — Release", "v0.1.0"));
    }

    #[test]
    fn header_matches_version_at_end_of_line() {
        assert!(header_matches_version("## v0.1.0", "v0.1.0"));
    }

    #[test]
    fn header_matches_version_followed_by_dash() {
        assert!(header_matches_version("## v0.1.0-beta", "v0.1.0"));
    }

    #[test]
    fn header_does_not_match_when_absent() {
        assert!(!header_matches_version("## v0.2.0", "v0.1.0"));
    }

    // --- extract ---

    #[test]
    fn extract_returns_section_content() {
        let content =
            "# Release Notes\n\n## v0.1.0\n\nFirst release.\n\n## v0.2.0\n\nSecond release.";
        let result = extract("v0.1.0", content);
        assert!(result.contains("## v0.1.0"));
        assert!(result.contains("First release."));
        assert!(!result.contains("Second release."));
    }

    #[test]
    fn extract_returns_empty_for_missing_version() {
        let content = "## v0.1.0\n\nNotes.";
        assert_eq!(extract("v0.9.9", content), "");
    }

    #[test]
    fn extract_captures_last_section() {
        let content = "## v0.1.0\n\nFirst.\n\n## v0.2.0\n\nLast section.";
        let result = extract("v0.2.0", content);
        assert!(result.contains("Last section."));
    }

    #[test]
    fn extract_does_not_confuse_longer_version() {
        let content = "## v0.1.0\n\nShort.\n\n## v0.10.0\n\nLong version.";
        let result = extract("v0.1.0", content);
        assert!(result.contains("Short."));
        assert!(!result.contains("Long version."));
    }

    // --- run_impl ---

    #[test]
    fn run_impl_missing_version_errors() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args { version: None };
        let err = run_impl(&args, dir.path()).unwrap_err();
        assert!(err.contains("Usage:"));
    }

    #[test]
    fn run_impl_invalid_version_format_errors() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            version: Some("not-a-version".to_string()),
        };
        let err = run_impl(&args, dir.path()).unwrap_err();
        assert!(err.contains("invalid version format"));
    }

    #[test]
    fn run_impl_missing_release_notes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let err = run_impl(&args, dir.path()).unwrap_err();
        assert!(err.contains("RELEASE-NOTES.md"));
        assert!(err.contains("not found"));
    }

    #[test]
    fn run_impl_version_not_in_notes_errors() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nOnly this version.");
        let args = Args {
            version: Some("v9.9.9".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(err.contains("no section found"));
    }

    #[test]
    fn run_impl_happy_path_writes_file() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nFirst release content.");
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("Written to"));
        let out_path = root.join("tmp").join("release-notes-v0.1.0.md");
        assert!(out_path.exists());
        let contents = fs::read_to_string(&out_path).unwrap();
        assert!(contents.contains("First release content."));
    }

    #[test]
    fn run_impl_accepts_version_with_or_without_v_prefix() {
        let (_dir, root) = setup_repo_with_notes("## 0.5.0\n\nNotes here.");
        let args = Args {
            version: Some("0.5.0".to_string()),
        };
        let output = run_impl(&args, &root).unwrap();
        assert!(output.contains("release-notes-0.5.0.md"));
    }
}
