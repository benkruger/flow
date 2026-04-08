use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::Value;

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

/// Copy flow-phases.json to .flow-states/<branch>-phases.json.
pub fn freeze_phases(
    phases_json_path: &Path,
    project_root: &Path,
    branch: &str,
) -> std::io::Result<()> {
    let dest_dir = project_root.join(".flow-states");
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(format!("{}-phases.json", branch));
    std::fs::copy(phases_json_path, dest)?;
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
pub fn find_state_files(root: &Path, branch: &str) -> Vec<(PathBuf, Value, String)> {
    let state_dir = root.join(".flow-states");

    // Exact match
    let exact_path = state_dir.join(format!("{}.json", branch));
    if exact_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&exact_path) {
            if let Ok(state) = serde_json::from_str::<Value>(&content) {
                return vec![(exact_path, state, branch.to_string())];
            }
        }
        return vec![];
    }

    if !state_dir.is_dir() {
        return vec![];
    }

    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        paths.sort_by_key(|e| e.file_name());

        for entry in paths {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".json") {
                continue;
            }
            if name_str.ends_with("-phases.json") {
                continue;
            }
            if name_str == "orchestrate.json" {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(state) = serde_json::from_str::<Value>(&content) {
                    let stem = name_str.trim_end_matches(".json").to_string();
                    results.push((entry.path(), state, stem));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- Constants ---

    #[test]
    fn phase_order_has_six_phases() {
        assert_eq!(PHASE_ORDER.len(), 6);
        assert_eq!(PHASE_ORDER[0], "flow-start");
        assert_eq!(PHASE_ORDER[5], "flow-complete");
    }

    #[test]
    fn phase_names_match_order() {
        let names = phase_names();
        assert_eq!(names.get("flow-start").unwrap(), "Start");
        assert_eq!(names.get("flow-code-review").unwrap(), "Code Review");
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn phase_numbers_are_one_indexed() {
        let nums = phase_numbers();
        assert_eq!(*nums.get("flow-start").unwrap(), 1);
        assert_eq!(*nums.get("flow-complete").unwrap(), 6);
    }

    #[test]
    fn commands_map_all_phases() {
        let cmds = commands();
        assert_eq!(cmds.get("flow-start").unwrap(), "/flow:flow-start");
        assert_eq!(cmds.get("flow-complete").unwrap(), "/flow:flow-complete");
        assert_eq!(cmds.len(), 6);
    }

    #[test]
    fn auto_skills_has_seven_entries() {
        let skills = auto_skills();
        assert_eq!(skills.len(), 7);
        assert!(matches!(
            skills.get("flow-abort").unwrap(),
            SkillConfig::Simple(s) if s == "auto"
        ));
        assert!(matches!(
            skills.get("flow-code").unwrap(),
            SkillConfig::Detailed(_)
        ));
    }

    // --- load_phase_config ---

    #[test]
    fn load_phase_config_from_real_file() {
        // Find the flow-phases.json relative to the test
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = PathBuf::from(manifest_dir).join("flow-phases.json");
        let config = load_phase_config(&path).unwrap();
        assert_eq!(config.order.len(), 6);
        assert_eq!(config.order[0], "flow-start");
        assert_eq!(config.names.get("flow-plan").unwrap(), "Plan");
        assert_eq!(config.commands.get("flow-code").unwrap(), "/flow:flow-code");
        assert_eq!(*config.numbers.get("flow-complete").unwrap(), 6);
    }

    #[test]
    fn load_phase_config_custom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("phases.json");
        fs::write(
            &path,
            r#"{
                "order": ["alpha", "beta"],
                "phases": {
                    "alpha": {"name": "Alpha", "command": "/test:alpha", "can_return_to": []},
                    "beta": {"name": "Beta", "command": "/test:beta", "can_return_to": ["alpha"]}
                }
            }"#,
        )
        .unwrap();

        let config = load_phase_config(&path).unwrap();
        assert_eq!(config.order, vec!["alpha", "beta"]);
        assert_eq!(config.names.get("alpha").unwrap(), "Alpha");
        assert_eq!(config.commands.get("beta").unwrap(), "/test:beta");
        assert_eq!(*config.numbers.get("alpha").unwrap(), 1);
        assert_eq!(*config.numbers.get("beta").unwrap(), 2);
    }

    #[test]
    fn load_phase_config_missing_file() {
        let result = load_phase_config(Path::new("/nonexistent/phases.json"));
        assert!(result.is_err());
    }

    // --- freeze_phases ---

    #[test]
    fn freeze_phases_copies_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("flow-phases.json");
        fs::write(&src, r#"{"order": [], "phases": {}}"#).unwrap();

        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();

        freeze_phases(&src, &project, "my-feature").unwrap();

        let dest = project.join(".flow-states").join("my-feature-phases.json");
        assert!(dest.exists());
        let content = fs::read_to_string(&dest).unwrap();
        assert!(content.contains("order"));
    }

    // --- build_initial_phases ---

    #[test]
    fn build_initial_phases_first_is_in_progress() {
        let phases = build_initial_phases("2026-01-01T00:00:00-08:00");
        let start = phases.get(&Phase::FlowStart).unwrap();
        assert_eq!(start.status, PhaseStatus::InProgress);
        assert_eq!(start.started_at, Some("2026-01-01T00:00:00-08:00".into()));
        assert_eq!(start.visit_count, 1);
    }

    #[test]
    fn build_initial_phases_rest_are_pending() {
        let phases = build_initial_phases("2026-01-01T00:00:00-08:00");
        let plan = phases.get(&Phase::FlowPlan).unwrap();
        assert_eq!(plan.status, PhaseStatus::Pending);
        assert!(plan.started_at.is_none());
        assert_eq!(plan.visit_count, 0);

        let complete = phases.get(&Phase::FlowComplete).unwrap();
        assert_eq!(complete.status, PhaseStatus::Pending);
    }

    #[test]
    fn build_initial_phases_has_six_entries() {
        let phases = build_initial_phases("2026-01-01T00:00:00-08:00");
        assert_eq!(phases.len(), 6);
    }

    #[test]
    fn build_initial_phases_preserves_insertion_order() {
        let phases = build_initial_phases("2026-01-01T00:00:00-08:00");
        let keys: Vec<&Phase> = phases.keys().collect();
        assert_eq!(
            keys,
            vec![
                &Phase::FlowStart,
                &Phase::FlowPlan,
                &Phase::FlowCode,
                &Phase::FlowCodeReview,
                &Phase::FlowLearn,
                &Phase::FlowComplete,
            ]
        );
    }

    #[test]
    fn auto_skills_preserves_insertion_order() {
        let skills = auto_skills();
        let keys: Vec<&String> = skills.keys().collect();
        assert_eq!(
            keys,
            vec![
                "flow-start",
                "flow-plan",
                "flow-code",
                "flow-code-review",
                "flow-learn",
                "flow-complete",
                "flow-abort",
            ]
        );
    }

    // --- find_state_files ---

    #[test]
    fn find_state_files_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("my-feature.json"),
            r#"{"branch": "my-feature"}"#,
        )
        .unwrap();

        let results = find_state_files(dir.path(), "my-feature");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "my-feature");
    }

    #[test]
    fn find_state_files_no_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = find_state_files(dir.path(), "main");
        assert!(results.is_empty());
    }

    #[test]
    fn find_state_files_fallback_scan() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-xyz.json"),
            r#"{"branch": "feature-xyz"}"#,
        )
        .unwrap();

        let results = find_state_files(dir.path(), "main");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "feature-xyz");
    }

    #[test]
    fn find_state_files_skips_phases_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-x.json"),
            r#"{"branch": "feature-x"}"#,
        )
        .unwrap();
        fs::write(state_dir.join("feature-x-phases.json"), r#"{"order": []}"#).unwrap();

        let results = find_state_files(dir.path(), "main");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "feature-x");
    }

    #[test]
    fn find_state_files_skips_orchestrate() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(
            state_dir.join("feature-x.json"),
            r#"{"branch": "feature-x"}"#,
        )
        .unwrap();
        fs::write(
            state_dir.join("orchestrate.json"),
            r#"{"status": "in_progress"}"#,
        )
        .unwrap();

        let results = find_state_files(dir.path(), "main");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "feature-x");
    }

    #[test]
    fn find_state_files_skips_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("bad.json"), "{corrupt").unwrap();
        fs::write(state_dir.join("good.json"), r#"{"branch": "good"}"#).unwrap();

        let results = find_state_files(dir.path(), "main");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "good");
    }

    #[test]
    fn find_state_files_corrupt_exact_no_fallthrough() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("main.json"), "{corrupt").unwrap();
        fs::write(state_dir.join("other.json"), r#"{"branch": "other"}"#).unwrap();

        let results = find_state_files(dir.path(), "main");
        assert!(results.is_empty());
    }

    // --- read_flow_json ---

    #[test]
    fn read_flow_json_valid() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".flow.json"),
            r#"{"version": "1.0.0", "tab_color": [255, 0, 0]}"#,
        )
        .unwrap();

        let result = read_flow_json(Some(dir.path()));
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["version"], "1.0.0");
    }

    #[test]
    fn read_flow_json_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_flow_json(Some(dir.path())).is_none());
    }

    #[test]
    fn read_flow_json_invalid() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".flow.json"), "{bad json").unwrap();
        assert!(read_flow_json(Some(dir.path())).is_none());
    }
}
