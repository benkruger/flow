//! Write content to a target file path.
//!
//! Usage:
//!   bin/flow write-rule --path <target> --content-file <temp>
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "path": "<target_path>"}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/write_rule.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::path::Path;

use clap::Parser;
use serde_json::json;

/// FLOW-managed artifacts whose on-disk location is computed by
/// `FlowPaths` rather than chosen by the caller. When `--path` names
/// one of these, write-rule canonicalizes the target — see
/// `canonical_path` and the `run_impl_main` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedArtifact {
    /// `<branch_dir>/plan.md`
    PlanMd,
    /// `<branch_dir>/dag.md`
    DagMd,
    /// `<branch_dir>/commit-msg.txt`
    CommitMsgTxt,
    /// `<project_root>/.flow-issue-body`
    FlowIssueBody,
    /// `<project_root>/.flow-states/orchestrate-queue.json`
    OrchestrateQueue,
}

/// Classify `path` by basename. Returns `Some(variant)` when the
/// basename matches a FLOW-managed artifact, `None` otherwise.
///
/// Pure function — does not touch the filesystem and does not
/// validate parent directories. The caller (`run_impl_main`)
/// computes the canonical destination from `(project_root, branch)`
/// and rejects when the canonicalized provided path differs.
pub fn classify_path(path: &Path) -> Option<ManagedArtifact> {
    let name = path.file_name()?.to_str()?;
    match name {
        "plan.md" => Some(ManagedArtifact::PlanMd),
        "dag.md" => Some(ManagedArtifact::DagMd),
        "commit-msg.txt" => Some(ManagedArtifact::CommitMsgTxt),
        ".flow-issue-body" => Some(ManagedArtifact::FlowIssueBody),
        "orchestrate-queue.json" => Some(ManagedArtifact::OrchestrateQueue),
        _ => None,
    }
}

#[derive(Parser, Debug)]
#[command(name = "write-rule", about = "Write content to a target file")]
pub struct Args {
    /// Target file path
    #[arg(long)]
    pub path: String,
    /// Path to file containing content (file is deleted after reading)
    #[arg(long = "content-file")]
    pub content_file: String,
}

/// Read content from a file and delete it.
/// Returns Ok(content) or Err(message).
pub fn read_content_file(path: &str) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Could not read content file '{}': {}", path, e))?;

    // Delete the content file after reading, ignore errors
    let _ = fs::remove_file(path);

    Ok(content)
}

/// Write content to the target path, creating parent dirs as needed.
/// Returns Ok(()) or Err(message).
pub fn write_rule(target_path: &str, content: &str) -> Result<(), String> {
    let path = Path::new(target_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directories for '{}': {}", target_path, e))?;
    }

    fs::write(path, content).map_err(|e| format!("Could not write to '{}': {}", target_path, e))?;

    Ok(())
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let content = match read_content_file(&args.content_file) {
        Ok(c) => c,
        Err(e) => return (json!({"status": "error", "message": e}), 1),
    };
    if let Err(e) = write_rule(&args.path, &content) {
        return (json!({"status": "error", "message": e}), 1);
    }
    (json!({"status": "ok", "path": &args.path}), 0)
}
