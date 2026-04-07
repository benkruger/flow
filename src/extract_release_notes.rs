//! Port of lib/extract-release-notes.py — extract release notes for a version.
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
#[command(name = "extract-release-notes", about = "Extract release notes for a version")]
pub struct Args {
    /// Version to extract (e.g. v0.2.0 or 0.2.0)
    pub version: Option<String>,
}

/// Extract the release notes section for a given version from content.
///
/// Finds the `## ` header line containing the version string, collects
/// all lines until the next `## ` header, and returns the trimmed result.
/// Returns an empty string if the version is not found as a header.
pub fn extract(version: &str, content: &str) -> String {
    let mut section: Vec<&str> = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.starts_with("## ") && line.contains(version) {
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

    let content =
        fs::read_to_string(&notes_file).map_err(|e| format!("Error reading {}: {}", notes_file.display(), e))?;

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
