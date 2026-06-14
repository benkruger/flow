//! `bin/flow resolve-skill-mode` — the single tested source of truth
//! for resolving the autonomy mode of any FLOW skill.
//!
//! Every skill's `## Mode Resolution` section calls this subcommand as
//! the single place that reads `skills.<name>` from the state file.
//! Given `--skill` (one of [`ALLOWED_SKILLS`]) and an optional
//! `--branch` override, it reads the block-shape config object
//! (`{"commit": .., "continue": ..}`) the `.flow.json`-seeded state
//! file carries, normalizes each axis, clamps it to the
//! `{auto, manual}` set, and returns a deterministic
//! `{"status":"ok","commit":..,"continue":..}`.
//!
//! Only the block (object) shape is parsed. A bare-string
//! `skills.<name>` entry, a missing entry, or a wrong-type entry
//! resolves each axis to the per-skill default — every skill is
//! `manual` on both axes, the conservative direction that asks the
//! user before proceeding. See [`default_mode`].
//!
//! Read-only: no `cwd_scope::enforce` call. Per
//! `.claude/rules/external-input-validation.md` and
//! `.claude/rules/branch-path-safety.md`, the `--branch` override is
//! untrusted shell input and routes through `FlowPaths::try_new` so a
//! slash-containing, empty, or traversal branch surfaces as a
//! structured error rather than a panic. Per
//! `.claude/rules/security-gates.md`, `--skill` and each resolved
//! axis value are normalized (NUL-stripped, trimmed, ASCII-lowercased
//! via `normalize_gate_input`) and checked against a positive
//! allowlist — `--skill` against [`ALLOWED_SKILLS`], each axis value
//! against `MODE_VALUES`. The `skills.<name>` object key is matched
//! case-insensitively against the normalized skill name — both sides
//! of the lookup are normalized so a hand-edited `.flow.json` with a
//! mixed-case skill key still resolves to its configured mode.
//!
//! `run_impl` returns `Value` unconditionally — every failure mode is
//! a structured `{"status":"error",...}` payload or a fallback, so
//! there is no infrastructure-failure `Err` path and the paired
//! `run_impl_main` wraps as `(value, 0)` per the "Exit code
//! convention for business errors" in `.claude/rules/rust-patterns.md`.
//!
//! Tests live at `tests/resolve_skill_mode.rs`.

use std::fs;
use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;

/// CLI args for `bin/flow resolve-skill-mode`.
#[derive(Parser, Debug)]
#[command(
    name = "resolve-skill-mode",
    about = "Resolve the configured autonomy mode of a FLOW skill"
)]
pub struct Args {
    /// Skill whose mode to resolve — one of [`ALLOWED_SKILLS`].
    #[arg(long)]
    pub skill: String,

    /// Override branch for state file lookup.
    #[arg(long)]
    pub branch: Option<String>,
}

/// The skills `resolve-skill-mode` answers for. A positive allowlist —
/// anything else is rejected with a structured error so a future
/// skill name added to the domain cannot silently pass the gate.
pub const ALLOWED_SKILLS: &[&str] = &[
    "flow-start",
    "flow-code",
    "flow-review",
    "flow-complete",
    "flow-abort",
];

/// Conservative fallback mode (`"manual"`) for callers that need the
/// `flow-complete` default before the irreversible Complete merge.
/// Consumed by the Complete-phase modules (`complete_merge.rs`,
/// `complete_preflight.rs`) when no state file is available. The
/// resolver's own per-skill fallback matrix lives in [`default_mode`].
pub const FALLBACK_MODE: &str = "manual";

/// Normalize a gate input before an allowlist comparison: strip NUL
/// bytes, trim surrounding whitespace, lowercase with ASCII
/// semantics. Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing". Shared by both gates in this module: `--skill` against
/// [`ALLOWED_SKILLS`], and each resolved axis value against
/// `MODE_VALUES`. The allowlist entries are already lowercase and
/// trimmed, so normalization runs on the caller side only.
pub fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Valid resolved modes. [`resolve`] normalizes each `skills.<skill>`
/// axis value and clamps anything outside this set to the per-skill
/// default, so callers can rely on each axis being exactly `"auto"`
/// or `"manual"`.
const MODE_VALUES: &[&str] = &["auto", "manual"];

/// Per-skill default `(commit, continue)` mode. Every skill defaults
/// to `manual` on both axes — the conservative direction that asks the
/// user before proceeding. Applied whenever the `skills.<skill>` config
/// is missing, the wrong type, a bare string, or carries an unparseable
/// axis value. The parameter is retained so a future skill that needs a
/// non-default mode can fork on the skill name.
fn default_mode(_skill: &str) -> (&'static str, &'static str) {
    ("manual", "manual")
}

/// Resolve one axis (`"commit"` or `"continue"`) of a `skills.<skill>`
/// entry. Only the object shape carries axis values: a bare string,
/// missing entry, or wrong-type entry yields the empty string, which
/// — like any value outside `MODE_VALUES` after [`normalize_gate_input`]
/// — clamps to `default`. The returned value is therefore always
/// exactly `"auto"` or `"manual"`.
fn resolve_axis(entry: Option<&Value>, axis: &str, default: &str) -> String {
    let raw = entry
        .and_then(|e| e.as_object())
        .and_then(|o| o.get(axis))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let normalized = normalize_gate_input(raw);
    if MODE_VALUES.contains(&normalized.as_str()) {
        normalized
    } else {
        default.to_string()
    }
}

/// Resolve the `(commit, continue)` mode for `skill` from a parsed
/// state file value.
///
/// `skill` is normalized via [`normalize_gate_input`], and the
/// `skills` object key is matched case-insensitively against that
/// normalized form — both sides of the comparison are normalized per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing", so
/// a hand-edited `.flow.json` carrying a mixed-case skill key still
/// resolves to its configured mode instead of silently falling to the
/// per-skill default.
///
/// Reads `commit` and `continue` from the matched `skills.<skill>`
/// object (`{"commit": .., "continue": ..}` or `{"continue": ..}`).
/// Every non-object shape — a bare string, missing `skills` key, a
/// `skills` value that is not an object, no matching entry,
/// `null`/number/array/bool entry — yields the per-skill
/// [`default_mode`] for both axes. An object missing one axis (or
/// carrying a non-string / unparseable value for it) takes the
/// default for that axis only. Each axis is normalized via
/// [`normalize_gate_input`] and clamped to `MODE_VALUES`, so the
/// returned pair is always exactly
/// `("auto"|"manual", "auto"|"manual")`.
pub fn resolve(state: &Value, skill: &str) -> (String, String) {
    let skill = normalize_gate_input(skill);
    let (default_commit, default_continue) = default_mode(&skill);
    let entry = state
        .get("skills")
        .and_then(|s| s.as_object())
        .and_then(|skills| {
            skills
                .iter()
                .find(|&(k, _)| normalize_gate_input(k) == skill)
                .map(|(_, v)| v)
        });
    (
        resolve_axis(entry, "commit", default_commit),
        resolve_axis(entry, "continue", default_continue),
    )
}

/// Resolve the autonomy mode for `args.skill` and return a structured
/// JSON payload.
///
/// Outcomes:
/// - `--skill` outside [`ALLOWED_SKILLS`] →
///   `{"status":"error","reason":"invalid_skill",...}`
/// - `--branch` (or the resolved current branch) fails
///   `FlowPaths::try_new` →
///   `{"status":"error","reason":"invalid_branch",...}`
/// - no current branch and no override (detached HEAD / non-git cwd)
///   → `{"status":"ok","commit":<default>,"continue":<default>}` —
///   no active flow, per-skill default
/// - state file missing / empty / non-JSON / non-object root →
///   `{"status":"ok",...}` with the per-skill default
/// - state file parses → `{"status":"ok","commit":..,"continue":..}`
///   via [`resolve`]
pub fn run_impl(args: &Args, root: &Path) -> Value {
    let skill = normalize_gate_input(&args.skill);
    if !ALLOWED_SKILLS.contains(&skill.as_str()) {
        return json!({
            "status": "error",
            "reason": "invalid_skill",
            "message": format!(
                "--skill must be one of {:?}, got {:?}",
                ALLOWED_SKILLS, args.skill
            ),
        });
    }
    let (commit, cont) = match resolve_branch(args.branch.as_deref(), root) {
        Some(branch) => {
            let paths = match FlowPaths::try_new(root, &branch) {
                Some(p) => p,
                None => {
                    return json!({
                        "status": "error",
                        "reason": "invalid_branch",
                        "message": format!(
                            "invalid branch {:?}: must be non-empty and contain no '/' or NUL",
                            branch
                        ),
                    });
                }
            };
            match fs::read_to_string(paths.state_file()) {
                Ok(content) => match serde_json::from_str::<Value>(&content) {
                    Ok(state) => resolve(&state, &skill),
                    Err(_) => resolve(&Value::Null, &skill),
                },
                Err(_) => resolve(&Value::Null, &skill),
            }
        }
        None => resolve(&Value::Null, &skill),
    };
    json!({"status": "ok", "commit": commit, "continue": cont})
}

/// Main-arm dispatcher. `resolve-skill-mode` has no
/// infrastructure-failure path — every outcome is a structured JSON
/// payload — so the exit code is always `0` per the "Exit code
/// convention for business errors" in `.claude/rules/rust-patterns.md`.
/// Callers parse the `status` field to distinguish success from error.
pub fn run_impl_main(args: &Args, root: &Path) -> (Value, i32) {
    (run_impl(args, root), 0)
}
