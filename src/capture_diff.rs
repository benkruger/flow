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
//!
//! Optionally, one or more `--family <pathspec>` arguments write a
//! whitespace-filtered diff scoped to each pathspec to
//! `.flow-states/<branch>/substantive-diff-<sanitized>.diff` and record
//! the paths in the success envelope's `family_slices` array. The
//! flow-review read-overflow remediation uses this to slice the diff
//! per directory family so each documentation-agent re-invocation reads
//! a bounded per-family file rather than the whole substantive diff.

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
    /// Optional, repeatable file-family pathspec. For each `--family
    /// <pathspec>`, capture-diff writes a whitespace-filtered diff
    /// scoped to that pathspec to
    /// `<branch_dir>/substantive-diff-<sanitized>.diff` and records the
    /// `(family, path)` pair in the success envelope's `family_slices`
    /// array. The flow-review read-overflow remediation passes one
    /// `--family` per directory family so each documentation-agent
    /// re-invocation reads a bounded per-family slice instead of the
    /// whole substantive diff. Each value is validated through
    /// `is_safe_family` before it reaches the `git diff -- <pathspec>`
    /// subprocess and the output filename, per
    /// `.claude/rules/external-input-path-construction.md`.
    #[arg(long)]
    pub family: Vec<String>,
}

/// Run capture-diff against an explicit `root` and `cwd`.
///
/// Validates `branch` via `FlowPaths::try_new`, runs `git diff
/// origin/<base>...HEAD` in `cwd` (full, and `-w` substantive), plus
/// one `-w … -- <pathspec>` diff per `--family`, and writes every
/// result into `<root>/.flow-states/<branch>/`. Returns a `(Value,
/// i32)` envelope where exit code is always `0` per the FLOW
/// business-error convention; callers parse the `status` field to
/// distinguish success from failure.
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
    // Validate every family pathspec BEFORE any subprocess or write
    // runs, so an invalid family aborts with the business-error envelope
    // without spawning git or touching the filesystem.
    for fam in &args.family {
        if !is_safe_family(fam) {
            return Err(format!("invalid family pathspec: {:?}", fam));
        }
    }

    let diff_range = format!("origin/{}...HEAD", args.base);
    // Collect every diff through a single `?` so the production code has
    // one error-propagation point. The first two argvs are the full
    // diff and the whitespace-filtered substantive diff (same range,
    // only `-w` differs); each remaining argv is the substantive diff
    // scoped to one `--family` pathspec. Folding the family diffs into
    // the same collect means there is one Err arm — reachable when the
    // base ref is unknown and the first `git diff` fails — rather than a
    // second per-family arm whose failure is unreachable once the range
    // is known-valid.
    let mut diff_argvs: Vec<Vec<String>> = vec![
        vec![diff_range.clone()],
        vec!["-w".to_string(), diff_range.clone()],
    ];
    for fam in &args.family {
        diff_argvs.push(vec![
            "-w".to_string(),
            diff_range.clone(),
            "--".to_string(),
            fam.clone(),
        ]);
    }
    let diffs = diff_argvs
        .iter()
        .map(|argv| {
            let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
            git_diff(cwd, &borrowed)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let full_path = paths.branch_dir().join("full-diff.diff");
    let sub_path = paths.branch_dir().join("substantive-diff.diff");
    std::fs::write(&full_path, &diffs[0]).map_err(|e| format!("write full-diff: {}", e))?;
    std::fs::write(&sub_path, &diffs[1]).map_err(|e| format!("write substantive-diff: {}", e))?;

    // Per-family slices share the branch dir. The filename fragment is
    // a single path component (separators folded to `_`), so it cannot
    // escape the branch dir even before the `is_safe_family` traversal
    // check above.
    let mut family_slices: Vec<Value> = Vec::with_capacity(args.family.len());
    for (i, fam) in args.family.iter().enumerate() {
        let fam_name = family_filename_component(fam);
        let fam_path = paths
            .branch_dir()
            .join(format!("substantive-diff-{}.diff", fam_name));
        std::fs::write(&fam_path, &diffs[2 + i])
            .map_err(|e| format!("write family diff {:?}: {}", fam, e))?;
        family_slices.push(json!({
            "family": fam,
            "path": fam_path.to_string_lossy(),
        }));
    }

    let mut envelope = json!({
        "status": "ok",
        "full": full_path.to_string_lossy(),
        "substantive": sub_path.to_string_lossy(),
        "branch": args.branch,
    });
    // Omit `family_slices` entirely when no `--family` was passed so the
    // envelope stays byte-compatible with callers that never slice
    // (e.g. flow-learn captures the substantive diff with no family).
    if !family_slices.is_empty() {
        envelope["family_slices"] = Value::Array(family_slices);
    }
    Ok(envelope)
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

/// Validate a `--family` pathspec before it reaches the `git diff --
/// <pathspec>` subprocess argument and the output filename. Per
/// `.claude/rules/external-input-path-construction.md`, every CLI
/// string flowing into a subprocess argument or a constructed path
/// needs a positive validator. Rejects:
///
/// - empty (no pathspec to scope on),
/// - NUL (`\0`), newline (`\n`), or carriage return (`\r`) — bytes
///   that would corrupt the output filename or smuggle content into
///   the diff range,
/// - any `.` or `..` path component — directory traversal, so a
///   crafted family cannot escape the branch dir via the filename,
/// - a leading `:` — git pathspec magic (`:/`, `:(exclude)…`) that
///   would change how git interprets the argument.
///
/// A trailing-slash directory pathspec like `src/` is accepted: its
/// components are `src` and an empty trailing segment, neither of
/// which is `.` or `..`. The pathspec is passed to git as a literal
/// `argv` element via `Command::arg` (never through a shell), so the
/// only structural concern is git pathspec magic, which the leading-`:`
/// rejection covers — no shell-escape function is required.
fn is_safe_family(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\0')
        && !s.contains('\n')
        && !s.contains('\r')
        && !s.starts_with(':')
        && !s.split('/').any(|c| c == "." || c == "..")
}

/// Derive a filesystem-safe, single-path-component filename fragment
/// from a validated family pathspec. `is_safe_family` has already
/// rejected NUL, newline/CR, `.`/`..` components, and a leading `:`, so
/// the only transformation needed is to fold the path separators into
/// `_` — splitting on `/`, dropping empty segments (the trailing slash
/// of a directory pathspec like `src/`), and rejoining with `_`. The
/// result is a single component with no `/`, so joining it onto the
/// branch dir cannot escape: `src/` -> `src`, `a/b/` -> `a_b`.
fn family_filename_component(s: &str) -> String {
    s.split('/')
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("_")
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
