//! Insert or replace FLOW priming content in a project's CLAUDE.md.
//!
//! Reads
//! `frameworks/<name>/priming.md` and inserts it between
//! `<!-- FLOW:BEGIN -->` / `<!-- FLOW:END -->` markers in the target
//! project's CLAUDE.md. Idempotent — re-running replaces existing
//! primed content.
//!
//! Usage: `bin/flow prime-project <project_root> --framework <name>`
//!
//! Output (JSON to stdout):
//!   `{"status": "ok", "framework": "...", "project_root": "..."}`
//!   `{"status": "error", "message": "..."}`

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use clap::Args as ClapArgs;
use serde_json::{json, Value};

use crate::utils::frameworks_dir;

const MARKER_BEGIN: &str = "<!-- FLOW:BEGIN -->";
const MARKER_END: &str = "<!-- FLOW:END -->";

#[derive(ClapArgs)]
pub struct Args {
    /// Project root directory (contains CLAUDE.md)
    pub project_root: String,

    /// Framework name (rails, python, ios, go, rust)
    #[arg(long)]
    pub framework: String,
}

/// Insert or replace priming content in a project's CLAUDE.md.
///
/// Returns a JSON Value with the same `status`, `replaced`, and
/// `message` fields the prime skill expects when reporting the
/// outcome to the user.
///
/// # Marker handling
///
/// Marker detection uses byte offsets. The markers
/// (`<!-- FLOW:BEGIN -->` / `<!-- FLOW:END -->`) are ASCII only, so
/// byte-index slicing never splits a multi-byte UTF-8 character. This
/// invariant is asserted via a debug_assert for defense in depth.
pub fn prime(project_root: &Path, framework: &str, fw_dir: &Path) -> Value {
    let claude_md = project_root.join("CLAUDE.md");
    if !claude_md.exists() {
        return json!({
            "status": "error",
            "message": "CLAUDE.md not found in project root",
        });
    }

    let priming_path = fw_dir.join(framework).join("priming.md");
    if !priming_path.exists() {
        return json!({
            "status": "error",
            "message": format!("Framework not found: {}", framework),
        });
    }

    let priming_content = match fs::read_to_string(&priming_path) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not read priming.md: {}", e),
            })
        }
    };

    let existing_content = match fs::read_to_string(&claude_md) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not read CLAUDE.md: {}", e),
            })
        }
    };

    // ASCII-only markers make byte-index slicing safe.
    debug_assert!(MARKER_BEGIN.is_ascii() && MARKER_END.is_ascii());

    let block = format!("{}\n\n{}\n{}\n", MARKER_BEGIN, priming_content, MARKER_END);

    let new_content = match (
        existing_content.find(MARKER_BEGIN),
        existing_content.find(MARKER_END),
    ) {
        (Some(begin_index), Some(end_index)) => {
            let mut end = end_index + MARKER_END.len();
            if existing_content.as_bytes().get(end) == Some(&b'\n') {
                end += 1;
            }
            let mut out = String::with_capacity(existing_content.len() + block.len());
            out.push_str(&existing_content[..begin_index]);
            out.push_str(&block);
            out.push_str(&existing_content[end..]);
            out
        }
        _ => {
            let mut out = String::with_capacity(existing_content.len() + block.len() + 1);
            out.push_str(&existing_content);
            out.push('\n');
            out.push_str(&block);
            out
        }
    };

    if let Err(e) = fs::write(&claude_md, new_content) {
        return json!({
            "status": "error",
            "message": format!("Could not write CLAUDE.md: {}", e),
        });
    }

    json!({
        "status": "ok",
        "framework": framework,
        "project_root": project_root.display().to_string(),
    })
}

/// Build the CLI result as a JSON value. Returns `Err` when the result
/// `status` is `"error"` so `run` can exit non-zero while still
/// printing the JSON body.
pub fn run_impl(args: &Args) -> Result<Value, Value> {
    let project_root = PathBuf::from(&args.project_root);
    let fw_dir = match frameworks_dir() {
        Some(p) => p,
        None => {
            return Err(json!({
                "status": "error",
                "message": "Plugin root not found",
            }))
        }
    };
    let result = prime(&project_root, &args.framework, &fw_dir);
    if result.get("status").and_then(|v| v.as_str()) == Some("error") {
        Err(result)
    } else {
        Ok(result)
    }
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
        }
        Err(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
            process::exit(1);
        }
    }
}
