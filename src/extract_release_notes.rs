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
//!
//! Tests live at `tests/extract_release_notes.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::fs;
use std::path::Path;

use clap::Parser;
use regex::Regex;

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

/// Main-arm dispatch: accepts a resolved `plugin_root` Option directly.
/// Returns `(message, exit_code)`.
pub fn run_impl_main(args: &Args, plugin_root: Option<std::path::PathBuf>) -> (String, i32) {
    let repo_root = match plugin_root {
        Some(r) => r,
        None => return ("Error: could not find FLOW plugin root".to_string(), 1),
    };
    match run_impl(args, &repo_root) {
        Ok(output) => (output, 0),
        Err(e) => (e, 1),
    }
}
