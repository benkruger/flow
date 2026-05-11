//! Capture the worktree diff against `origin/<base>` for the Review
//! sub-agents.
//!
//! The `capture-diff` subcommand replaces the inline `git diff` the
//! flow-review skill previously embedded in each agent prompt. The
//! diff is captured once and written to canonical
//! `.flow-states/<branch>/full-diff.diff` and
//! `.flow-states/<branch>/substantive-diff.diff` files; agents read
//! the files via the Read tool instead of receiving the diff bytes
//! through their prompt. Keeps the parent skill's prompt budget
//! bounded as PR size grows so the four review agents do not
//! starve their own investigation budgets.

use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

use crate::flow_paths::FlowPaths;

/// CLI arguments for `bin/flow capture-diff`.
#[derive(clap::Parser, Debug)]
#[command(name = "capture-diff")]
pub struct Args {
    /// Branch name. Validated through `FlowPaths::try_new` per
    /// `.claude/rules/branch-path-safety.md` so a slash-containing
    /// or path-traversing branch cannot escape the per-branch
    /// subdirectory.
    #[arg(long)]
    pub branch: String,
    /// Base ref against which to compute the diff (e.g., `main`).
    /// Combined with `origin/<base>` to form the diff range
    /// `origin/<base>...HEAD`.
    #[arg(long)]
    pub base: String,
}

/// Run capture-diff against an explicit `root` and `cwd`.
///
/// Validates `branch` via `FlowPaths::try_new`, runs `git diff
/// origin/<base>...HEAD` in `cwd` (twice — once full, once with `-w`
/// to drop whitespace-only hunks), and writes both results into
/// `<root>/.flow-states/<branch>/`. Returns a `(Value, i32)` envelope
/// where exit code is always `0` per the FLOW business-error
/// convention; callers parse the `status` field to distinguish
/// success from failure.
pub fn run_impl(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    match capture(args, root, cwd) {
        Ok(envelope) => (envelope, 0),
        Err(msg) => (
            json!({
                "status": "error",
                "message": msg,
            }),
            0,
        ),
    }
}

/// Capture both diffs and write them, returning the success envelope
/// or a single error message. Collapses every error path through `?`
/// propagation so the production code has one error handler rather
/// than duplicated `match` arms at each fallible step.
fn capture(args: &Args, root: &Path, cwd: &Path) -> Result<Value, String> {
    let paths = FlowPaths::try_new(root, &args.branch)
        .ok_or_else(|| format!("invalid branch name: {:?}", args.branch))?;
    paths
        .ensure_branch_dir()
        .map_err(|e| format!("create branch dir: {}", e))?;
    if !is_safe_base(&args.base) {
        return Err(format!("invalid base ref: {:?}", args.base));
    }

    let diff_range = format!("origin/{}...HEAD", args.base);
    // Collect both diffs through a single `?` so the production code has
    // one error-propagation point. The two underlying `git diff`
    // invocations are structurally identical (same range, only `-w`
    // differs) — collapsing into one Err arm avoids a second arm whose
    // failure is reachable only via TOCTOU between consecutive subprocesses.
    let mut outputs = [&[diff_range.as_str()][..], &["-w", diff_range.as_str()][..]]
        .iter()
        .map(|argv| git_diff(cwd, argv))
        .collect::<Result<Vec<_>, _>>()?;
    let substantive = outputs.pop().expect("two diff outputs collected");
    let full = outputs.pop().expect("two diff outputs collected");

    let full_path = paths.branch_dir().join("full-diff.diff");
    let sub_path = paths.branch_dir().join("substantive-diff.diff");
    std::fs::write(&full_path, &full).map_err(|e| format!("write full-diff: {}", e))?;
    std::fs::write(&sub_path, &substantive)
        .map_err(|e| format!("write substantive-diff: {}", e))?;

    Ok(json!({
        "status": "ok",
        "full": full_path.to_string_lossy(),
        "substantive": sub_path.to_string_lossy(),
        "branch": args.branch,
    }))
}

/// Run `git diff` with the supplied args in `cwd`.
///
/// Returns the stdout bytes on success; the captured stderr on
/// failure (typically `unknown revision` when the base ref does not
/// exist on `origin`). Spawn failures surface as `spawn git: <io
/// error>` so a missing `git` binary is distinguishable from a
/// non-zero exit.
/// Validate a `--base` ref value before interpolating it into the git
/// diff range. Per `.claude/rules/external-input-path-construction.md`,
/// every CLI string that flows into `format!` or a subprocess argument
/// needs a positive validator. Rejects empty, NUL bytes, newlines,
/// path-separator slashes (other than `/` which is valid in remote-tracking
/// refs like `origin/main`... but `--base` is the simple branch component,
/// never with `origin/` prefix — capture_diff adds the prefix itself).
fn is_safe_base(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\0')
        && !s.contains('\n')
        && !s.contains('\r')
        && !s.contains(' ')
        && s != "."
        && s != ".."
}

fn git_diff(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("diff")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn git: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(stderr);
    }
    Ok(output.stdout)
}
