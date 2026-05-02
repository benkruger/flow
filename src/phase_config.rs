//! Phase configuration — loads `flow-phases.json`, builds the
//! initial phases map, and resolves state files for a branch.
//!
//! Tests live at `tests/phase_config.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::Value;

use crate::flow_paths::{FlowPaths, FlowStatesDir};
use crate::state::{Phase, PhaseState, PhaseStatus, SkillConfig};

/// Phase configuration loaded from flow-phases.json.
#[derive(Debug, Clone)]
pub struct PhaseConfig {
    pub order: Vec<String>,
    pub names: IndexMap<String, String>,
    pub numbers: IndexMap<String, usize>,
    pub commands: IndexMap<String, String>,
}

/// Phase order constant derived from flow-phases.json.
pub const PHASE_ORDER: &[&str] = &[
    "flow-start",
    "flow-plan",
    "flow-code",
    "flow-code-review",
    "flow-learn",
    "flow-complete",
];

/// Build the PHASE_NAMES map.
pub fn phase_names() -> IndexMap<String, String> {
    let mut m = IndexMap::new();
    m.insert("flow-start".into(), "Start".into());
    m.insert("flow-plan".into(), "Plan".into());
    m.insert("flow-code".into(), "Code".into());
    m.insert("flow-code-review".into(), "Code Review".into());
    m.insert("flow-learn".into(), "Learn".into());
    m.insert("flow-complete".into(), "Complete".into());
    m
}

/// Build the COMMANDS map.
pub fn commands() -> IndexMap<String, String> {
    let mut m = IndexMap::new();
    m.insert("flow-start".into(), "/flow:flow-start".into());
    m.insert("flow-plan".into(), "/flow:flow-plan".into());
    m.insert("flow-code".into(), "/flow:flow-code".into());
    m.insert("flow-code-review".into(), "/flow:flow-code-review".into());
    m.insert("flow-learn".into(), "/flow:flow-learn".into());
    m.insert("flow-complete".into(), "/flow:flow-complete".into());
    m
}

/// Single-lookup alternative to [`phase_numbers`] — avoids map allocation for per-call use.
/// Returns the 1-based phase number for a phase name, or 0 if not found.
pub fn phase_number(phase: &str) -> usize {
    PHASE_ORDER
        .iter()
        .position(|&p| p == phase)
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// Build the PHASE_NUMBER map (1-indexed).
pub fn phase_numbers() -> IndexMap<String, usize> {
    PHASE_ORDER
        .iter()
        .enumerate()
        .map(|(i, k)| (k.to_string(), i + 1))
        .collect()
}

/// Build the AUTO_SKILLS default configuration.
pub fn auto_skills() -> IndexMap<String, SkillConfig> {
    let mut m = IndexMap::new();
    let mut start = IndexMap::new();
    start.insert("continue".into(), "auto".into());
    m.insert("flow-start".into(), SkillConfig::Detailed(start));

    let mut plan = IndexMap::new();
    plan.insert("continue".into(), "auto".into());
    plan.insert("dag".into(), "auto".into());
    m.insert("flow-plan".into(), SkillConfig::Detailed(plan));

    let mut code = IndexMap::new();
    code.insert("commit".into(), "auto".into());
    code.insert("continue".into(), "auto".into());
    m.insert("flow-code".into(), SkillConfig::Detailed(code));

    let mut review = IndexMap::new();
    review.insert("commit".into(), "auto".into());
    review.insert("continue".into(), "auto".into());
    m.insert("flow-code-review".into(), SkillConfig::Detailed(review));

    let mut learn = IndexMap::new();
    learn.insert("commit".into(), "auto".into());
    learn.insert("continue".into(), "auto".into());
    m.insert("flow-learn".into(), SkillConfig::Detailed(learn));

    m.insert("flow-complete".into(), SkillConfig::Simple("auto".into()));
    m.insert("flow-abort".into(), SkillConfig::Simple("auto".into()));
    m
}

/// Load phase config from a JSON file, returning a PhaseConfig struct.
///
/// Works with both the canonical flow-phases.json and frozen per-branch copies.
pub fn load_phase_config(path: &Path) -> Result<PhaseConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    let data: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in {}: {}", path.display(), e))?;

    let order: Vec<String> = data
        .get("order")
        .and_then(|v| v.as_array())
        .ok_or("Missing 'order' array")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let phases = data
        .get("phases")
        .and_then(|v| v.as_object())
        .ok_or("Missing 'phases' object")?;

    let mut names = IndexMap::new();
    let mut cmds = IndexMap::new();
    let mut numbers = IndexMap::new();

    for (i, key) in order.iter().enumerate() {
        if let Some(phase) = phases.get(key).and_then(|v| v.as_object()) {
            if let Some(name) = phase.get("name").and_then(|v| v.as_str()) {
                names.insert(key.clone(), name.to_string());
            }
            if let Some(cmd) = phase.get("command").and_then(|v| v.as_str()) {
                cmds.insert(key.clone(), cmd.to_string());
            }
        }
        numbers.insert(key.clone(), i + 1);
    }

    Ok(PhaseConfig {
        order,
        names,
        numbers,
        commands: cmds,
    })
}

/// Copy flow-phases.json to `.flow-states/<branch>/phases.json`.
/// Ensures the branch directory exists before the copy so the
/// destination path is always writable; the file lives alongside
/// `state.json` and the rest of the per-branch artifacts under the
/// same `branch_dir()`.
pub fn freeze_phases(
    phases_json_path: &Path,
    project_root: &Path,
    branch: &str,
) -> std::io::Result<()> {
    let paths = FlowPaths::new(project_root, branch);
    paths.ensure_branch_dir()?;
    std::fs::copy(phases_json_path, paths.frozen_phases())?;
    Ok(())
}

/// Build the initial phases dict for a new state file.
///
/// The first phase in PHASE_ORDER is set to in_progress with timestamps
/// and visit_count=1. All other phases are set to pending with null
/// timestamps and visit_count=0.
pub fn build_initial_phases(current_time: &str) -> IndexMap<Phase, PhaseState> {
    let mut phases = IndexMap::new();
    let phase_variants = [
        Phase::FlowStart,
        Phase::FlowPlan,
        Phase::FlowCode,
        Phase::FlowCodeReview,
        Phase::FlowLearn,
        Phase::FlowComplete,
    ];
    let names = phase_names();

    for (i, &phase) in phase_variants.iter().enumerate() {
        let key = PHASE_ORDER[i];
        let name = names.get(key).cloned().unwrap_or_default();

        if i == 0 {
            phases.insert(
                phase,
                PhaseState {
                    name,
                    status: PhaseStatus::InProgress,
                    started_at: Some(current_time.to_string()),
                    completed_at: None,
                    session_started_at: Some(current_time.to_string()),
                    cumulative_seconds: 0,
                    visit_count: 1,
                },
            );
        } else {
            phases.insert(
                phase,
                PhaseState {
                    name,
                    status: PhaseStatus::Pending,
                    started_at: None,
                    completed_at: None,
                    session_started_at: None,
                    cumulative_seconds: 0,
                    visit_count: 0,
                },
            );
        }
    }
    phases
}

/// Find state file(s), trying exact branch match first.
///
/// Returns list of (PathBuf, Value, String) tuples: (path, state, branch_name).
/// Empty list = nothing found. Single item = unambiguous match.
/// Multiple items = caller must disambiguate.
///
/// A `branch` that fails `FlowPaths::is_valid_branch` (empty or
/// containing '/') skips the exact-match lookup and scans the
/// `.flow-states/` directory directly. This covers both the format-
/// status multi-flow fallback (which passes `""`) and users running
/// `bin/flow` subcommands on a slash-containing git branch
/// (`feature/foo`, `dependabot/*`) where FLOW has no state file.
pub fn find_state_files(root: &Path, branch: &str) -> Vec<(PathBuf, Value, String)> {
    let state_dir = FlowStatesDir::new(root).path().to_path_buf();

    // Exact match — skip when the branch isn't a valid FLOW branch
    // name (empty, or slash-containing). FlowPaths::try_new returns
    // None in those cases so we fall through to the directory scan.
    if let Some(paths) = FlowPaths::try_new(root, branch) {
        let exact_path = paths.state_file();
        if exact_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&exact_path) {
                if let Ok(state) = serde_json::from_str::<Value>(&content) {
                    return vec![(exact_path, state, branch.to_string())];
                }
            }
            return vec![];
        }
    }

    if !state_dir.is_dir() {
        return vec![];
    }

    // Discovery: every branch-scoped subdirectory under `.flow-states/`
    // that contains a readable `state.json`. Subdirectories without
    // `state.json` (transient cleanup remnants, future per-machine
    // tooling) and regular files at the root (e.g. `orchestrate.json`,
    // the start lock, plain stale flat-layout artifacts left by older
    // binaries) are skipped naturally because they fail the
    // `state.json` filter.
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        let mut subdirs: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .collect();
        subdirs.sort_by_key(|e| e.file_name());

        for entry in subdirs {
            let name = entry.file_name();
            let branch_name = name.to_string_lossy().into_owned();
            let state_path = entry.path().join("state.json");
            if !state_path.is_file() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&state_path) {
                if let Ok(state) = serde_json::from_str::<Value>(&content) {
                    results.push((state_path, state, branch_name));
                }
            }
        }
    }

    results
}

/// Read and parse .flow.json from the given root (or CWD).
///
/// Returns the parsed Value on success, or None if the file is missing
/// or contains invalid JSON.
pub fn read_flow_json(root: Option<&Path>) -> Option<Value> {
    let path = match root {
        Some(r) => r.join(".flow.json"),
        None => PathBuf::from(".flow.json"),
    };
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}
