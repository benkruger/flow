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

    let content = match fs::read_to_string(&notes_file) {
        Ok(c) => c,
        Err(e) => return Err(format!("Error reading {}: {}", notes_file.display(), e)),
    };

    let extracted = extract(version, &content);
    if extracted.is_empty() {
        return Err(format!("Error: no section found for version {}", version));
    }

    let out_dir = repo_root.join("tmp");
    if let Err(e) = fs::create_dir_all(&out_dir) {
        return Err(format!("Error creating tmp dir: {}", e));
    }

    let out_path = out_dir.join(format!("release-notes-{}.md", version));
    if let Err(e) = fs::write(&out_path, format!("{}\n", extracted)) {
        return Err(format!("Error writing {}: {}", out_path.display(), e));
    }

    Ok(format!("Written to {}", out_path.display()))
}

/// Testable variant of [`run`] that accepts an injectable plugin-root
/// resolver so unit tests can drive the `None` arm. The walk-up
/// fallback in production `plugin_root()` always finds a real repo
/// from inside flow's source tree, making the None arm unreachable
/// from any subprocess test launched here.
///
/// Returns `(message, exit_code)`.
pub fn run_with_plugin_root_fn<F: FnOnce() -> Option<std::path::PathBuf>>(
    args: &Args,
    plugin_root_fn: F,
) -> (String, i32) {
    let repo_root = match plugin_root_fn() {
        Some(r) => r,
        None => return ("Error: could not find FLOW plugin root".to_string(), 1),
    };
    match run_impl(args, &repo_root) {
        Ok(output) => (output, 0),
        Err(e) => (e, 1),
    }
}

pub fn run(args: Args) -> ! {
    let (msg, code) = run_with_plugin_root_fn(&args, plugin_root);
    crate::dispatch::dispatch_text(&msg, code)
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

    /// Exercises `run_with_plugin_root_fn`'s None arm.
    #[test]
    fn run_with_plugin_root_fn_none_returns_error_tuple() {
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let (msg, code) = run_with_plugin_root_fn(&args, || None);
        assert_eq!(code, 1);
        assert!(msg.contains("could not find FLOW plugin root"));
    }

    /// Exercises `run_with_plugin_root_fn`'s success path.
    #[test]
    fn run_with_plugin_root_fn_success_returns_written_message() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nFirst.");
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let root_clone = root.clone();
        let (msg, code) = run_with_plugin_root_fn(&args, move || Some(root_clone));
        assert_eq!(code, 0);
        assert!(msg.contains("Written to"));
    }

    /// Exercises `run_with_plugin_root_fn`'s Err path (run_impl Err).
    #[test]
    fn run_with_plugin_root_fn_err_path_returns_no_section_found() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nFirst.");
        let args = Args {
            version: Some("v9.9.9".to_string()),
        };
        let root_clone = root.clone();
        let (msg, code) = run_with_plugin_root_fn(&args, move || Some(root_clone));
        assert_eq!(code, 1);
        assert!(msg.contains("no section found"));
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

    /// Exercises line 97 — the read_to_string Err arm. When
    /// RELEASE-NOTES.md is a directory instead of a file, exists() is
    /// true but read_to_string fails with EISDIR.
    #[test]
    fn run_impl_release_notes_is_directory_returns_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Create RELEASE-NOTES.md as a DIRECTORY so .exists() returns
        // true but fs::read_to_string fails.
        std::fs::create_dir(root.join("RELEASE-NOTES.md")).unwrap();
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(
            err.contains("Error reading"),
            "expected read error message, got: {}",
            err
        );
    }

    /// Exercises line 112 — the fs::write Err arm. Pre-occupy the
    /// out_path as a directory so fs::write fails with EISDIR.
    #[test]
    fn run_impl_out_path_is_existing_directory_returns_write_error() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nContent.");
        // Pre-occupy the output path as a directory so write fails.
        let tmp = root.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir(tmp.join("release-notes-v0.1.0.md")).unwrap();
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(err.contains("Error writing"), "got: {}", err);
    }

    /// Exercises line 107 — the create_dir_all Err arm. When `tmp` is
    /// pre-occupied as a regular file, create_dir_all fails.
    #[test]
    fn run_impl_tmp_is_existing_file_returns_create_dir_error() {
        let (_dir, root) = setup_repo_with_notes("## v0.1.0\n\nContent.");
        // Pre-occupy `tmp` as a file so create_dir_all fails.
        std::fs::write(root.join("tmp"), "regular file").unwrap();
        let args = Args {
            version: Some("v0.1.0".to_string()),
        };
        let err = run_impl(&args, &root).unwrap_err();
        assert!(
            err.contains("Error creating tmp dir"),
            "expected create-dir error message, got: {}",
            err
        );
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
