//! Promote permissions from settings.local.json into settings.json.
//!
//! Reads
//! `.claude/settings.local.json`, merges new `permissions.allow` entries
//! into `.claude/settings.json`, deletes settings.local.json, and
//! outputs JSON.
//!
//! Usage: `bin/flow promote-permissions --worktree-path <path>`
//!
//! Output (JSON to stdout):
//!   `{"status": "skipped", "reason": "no_local_file"}`
//!   `{"status": "ok", "promoted": [...], "already_present": N}`
//!   `{"status": "error", "message": "..."}`
//!
//! Tests live at tests/promote_permissions.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use serde_json::{json, Value};

use crate::hooks::is_flow_active;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the worktree or project root
    #[arg(long = "worktree-path")]
    pub worktree_path: String,
    /// Acknowledge that the caller intends to mutate `.claude/settings.json`
    /// while a flow is active for the target worktree's branch.
    ///
    /// Default `false` — the active-flow gate inside `run_impl` returns
    /// `{"status":"skipped","reason":"active_flow"}` instead of merging,
    /// closing the subprocess-layer hole that lets a model bypass the
    /// "Never Edit Permissions Mid-Flow" rule via a direct subprocess
    /// call. The `flow-learn` skill (the only sanctioned mid-flow caller
    /// per `skills/flow-learn/SKILL.md` Step 4) passes `--confirm-on-flow-branch`
    /// to bypass the gate. Outside a flow the flag is a no-op.
    #[arg(long = "confirm-on-flow-branch", default_value_t = false)]
    pub confirm_on_flow_branch: bool,
}

/// Merge settings.local.json allow entries into settings.json.
///
/// Returns one of three result shapes: `skipped` (no local file present),
/// `ok` (merged successfully with the list of newly promoted entries),
/// or `error` (parse, write, or shape failure with a displayable message).
/// The local file is deleted on success; deletion failures are swallowed
/// because the next promote() call will retry.
pub fn promote(worktree_path: &Path) -> Value {
    let local_path = worktree_path.join(".claude").join("settings.local.json");
    let settings_path = worktree_path.join(".claude").join("settings.json");

    if !local_path.exists() {
        return json!({"status": "skipped", "reason": "no_local_file"});
    }

    let local_data: Value = match read_json(&local_path) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not parse settings.local.json: {}", e),
            })
        }
    };

    let local_allow: Vec<String> = local_data
        .get("permissions")
        .and_then(|v| v.get("allow"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !settings_path.exists() {
        return json!({
            "status": "error",
            "message": "settings.json does not exist",
        });
    }

    let mut settings_data: Value = match read_json(&settings_path) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Could not parse settings.json: {}", e),
            })
        }
    };

    let mut existing_allow: Vec<Value> = settings_data
        .get("permissions")
        .and_then(|v| v.get("allow"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut existing_set: HashSet<String> = existing_allow
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mut promoted: Vec<String> = Vec::new();
    let mut already_present: i64 = 0;
    for entry in local_allow {
        if existing_set.contains(&entry) {
            already_present += 1;
        } else {
            promoted.push(entry.clone());
            existing_allow.push(Value::String(entry.clone()));
            existing_set.insert(entry);
        }
    }

    if !(settings_data.is_object() || settings_data.is_null()) {
        return json!({
            "status": "error",
            "message": "settings.json is not a JSON object",
        });
    }

    // Guard both the top-level settings object and the nested `permissions`
    // value — if either is not an object, assigning `["permissions"]["allow"]`
    // would trigger a serde_json IndexMut panic. Replace a malformed
    // permissions value with a fresh empty object so the merge can proceed.
    if !matches!(settings_data.get("permissions"), Some(v) if v.is_object()) {
        settings_data["permissions"] = json!({});
    }
    settings_data["permissions"]["allow"] = Value::Array(existing_allow);

    // `serde_json::to_string_pretty` over an in-memory `Value` built
    // from the `json!()` macro cannot fail — the only error sources are
    // I/O on a custom Writer (we use String) and types that don't
    // implement Serialize (Value implements it). Use `expect` to drop
    // the unreachable Err arm rather than carry a dead branch.
    let serialized = serde_json::to_string_pretty(&settings_data)
        .expect("serializing an in-memory JSON Value to a String cannot fail");

    let mut bytes = serialized.into_bytes();
    bytes.push(b'\n');
    if let Err(e) = fs::write(&settings_path, &bytes) {
        return json!({
            "status": "error",
            "message": format!("Could not write settings.json: {}", e),
        });
    }

    // Best-effort cleanup: tolerate I/O errors here because the next
    // promote() call retries the merge and the deletion.
    let _ = fs::remove_file(&local_path);

    json!({
        "status": "ok",
        "promoted": promoted,
        "already_present": already_present,
    })
}

/// Read a JSON file and parse it. Bundles `io::Error` and
/// `serde_json::Error` into a single displayable error string so the
/// caller can emit a unified `"Could not parse <path>: <reason>"`
/// message without inspecting which layer failed.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// Active-flow gate for promote-permissions during a FLOW phase.
///
/// Closes the subprocess-layer hole the `validate-claude-paths` hook
/// leaves open — the hook does not protect `.claude/settings.json`, so
/// `bin/flow promote-permissions --worktree-path <worktree>` can
/// mutate the worktree's settings file mid-flow without the user
/// noticing. The gate enforces `.claude/rules/permissions.md` "Never
/// Edit Permissions Mid-Flow" mechanically: when `worktree_path`
/// resolves to `<main_root>/.worktrees/<branch>/` AND a state file
/// exists at `<main_root>/.flow-states/<branch>/state.json`, the
/// merge is skipped.
///
/// `--confirm-on-flow-branch` lifts the gate. `flow-learn` Step 4 —
/// the one sanctioned mid-flow caller — passes the flag so its
/// promotion runs to completion. A model that bypasses flow-learn
/// and constructs the flag itself is documented as the prose-only
/// limitation in the rule (mirroring the `_continue_pending=commit`
/// trust contract in concurrency-model.md).
///
/// Returns `Some(skipped_value)` when the gate fires, `None`
/// otherwise. Detection uses path-based heuristics — no git
/// subprocess — so a missing or unreachable git binary cannot
/// silently disable the gate.
///
/// Branch derivation matches `worktree_path_guard` in
/// `src/write_rule.rs`: extract the first segment after
/// `.worktrees/` in the resolved `worktree_path`, bypassing
/// `crate::hooks::detect_branch_from_path`'s `.git` walk-up so a
/// submodule subdirectory inside a worktree cannot return
/// `<branch>/<sub>` and silently disable the gate.
fn active_flow_gate(worktree_path: &Path, confirm: bool) -> Option<Value> {
    if confirm {
        return None;
    }
    // Mirror the write_rule guard: `.expect` on `current_dir()` is
    // intentional. A subprocess that cannot resolve its own cwd is
    // broken in ways the surrounding `promote()` call would already
    // fail on (it joins `worktree_path` against `.claude/`). Collapsing
    // the branch keeps the gate testable without a deleted-cwd fixture
    // per `.claude/rules/testability-means-simplicity.md`.
    let abs = if worktree_path.is_absolute() {
        worktree_path.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("subprocess cwd must be resolvable for promote-permissions")
            .join(worktree_path)
    };
    let mut current = abs.clone();
    let main_root = loop {
        if current.join(".flow-states").is_dir() {
            break current;
        }
        if !current.pop() {
            return None;
        }
    };
    let branch = worktree_branch_from_path(&abs)?;
    if !is_flow_active(&branch, &main_root) {
        return None;
    }
    Some(json!({
        "status": "skipped",
        "reason": "active_flow",
        "message": format!(
            "promote-permissions skipped: active flow on branch '{}' \
             at {}. Pass --confirm-on-flow-branch to override (per \
             .claude/rules/permissions.md \"Never Edit Permissions \
             Mid-Flow\"); flow-learn Step 4 passes the flag automatically.",
            branch,
            main_root.display()
        ),
        "branch": branch,
    }))
}

/// Extract the worktree branch from the first `.worktrees/<X>/` segment
/// in `path`. Mirrors `worktree_branch_from_path` in
/// `src/write_rule.rs`. Duplicated rather than promoted to a shared
/// helper because the loop is six lines and a new pub surface would
/// require its own consumer + test row per
/// `.claude/rules/test-placement.md` "Bright-line test for `pub`
/// additions".
///
/// Returns `None` when the path has no `.worktrees/<X>/` segment.
/// An empty-segment input (path ending exactly at `.worktrees/`)
/// returns `Some("")` and is rejected downstream by
/// `is_flow_active` via `FlowPaths::try_new` — collapsed so the
/// helper has no unreachable branch.
fn worktree_branch_from_path(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let pos = s.find(".worktrees/")?;
    let after = &s[pos + ".worktrees/".len()..];
    let segment = after
        .split('/')
        .next()
        .expect("str::split iterator is non-empty; next() always returns Some");
    Some(segment.to_string())
}

/// Build the CLI result as a JSON value.
///
/// Returns `Err` when the result `status` is `"error"` so `run` can
/// exit non-zero with JSON output, while keeping `ok`/`skipped` on
/// the `Ok` path.
pub fn run_impl(args: &Args) -> Result<Value, Value> {
    let worktree = PathBuf::from(&args.worktree_path);
    if let Some(skipped) = active_flow_gate(&worktree, args.confirm_on_flow_branch) {
        return Ok(skipped);
    }
    let result = promote(&worktree);
    if result.get("status").and_then(|v| v.as_str()) == Some("error") {
        Err(result)
    } else {
        Ok(result)
    }
}

/// Main-arm dispatch: returns (value, exit code).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match run_impl(args) {
        Ok(value) => (value, 0),
        Err(value) => (value, 1),
    }
}
