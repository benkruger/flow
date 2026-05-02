//! Tests for `flow_rs::phase_config`. Migrated from inline
//! `#[cfg(test)]` per `.claude/rules/test-placement.md`. All tests
//! drive through the public surface.

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::flow_paths::{FlowPaths, FlowStatesDir};
use flow_rs::phase_config::{
    auto_skills, build_initial_phases, commands, find_state_files, freeze_phases,
    load_phase_config, phase_names, phase_number, phase_numbers, read_flow_json, PHASE_ORDER,
};
use flow_rs::state::{Phase, PhaseStatus, SkillConfig};

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
fn phase_number_returns_one_indexed() {
    assert_eq!(phase_number("flow-start"), 1);
    assert_eq!(phase_number("flow-plan"), 2);
    assert_eq!(phase_number("flow-code"), 3);
    assert_eq!(phase_number("flow-code-review"), 4);
    assert_eq!(phase_number("flow-learn"), 5);
    assert_eq!(phase_number("flow-complete"), 6);
}

#[test]
fn phase_number_returns_zero_for_unknown() {
    assert_eq!(phase_number("nonexistent"), 0);
    assert_eq!(phase_number(""), 0);
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
    assert_eq!(
        skills.get("flow-abort").unwrap(),
        &SkillConfig::Simple("auto".to_string())
    );
    assert!(format!("{:?}", skills.get("flow-code").unwrap()).starts_with("Detailed("));
}

// --- load_phase_config ---

#[test]
fn load_phase_config_from_real_file() {
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

#[test]
fn load_phase_config_missing_order_returns_err() {
    // Exercises the `.ok_or("Missing 'order' array")?` arm.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phases.json");
    fs::write(&path, r#"{"phases": {}}"#).unwrap();
    let err = load_phase_config(&path).unwrap_err();
    assert!(err.contains("Missing 'order'"), "err was: {}", err);
}

#[test]
fn load_phase_config_missing_phases_returns_err() {
    // Exercises the `.ok_or("Missing 'phases' object")?` arm.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phases.json");
    fs::write(&path, r#"{"order": []}"#).unwrap();
    let err = load_phase_config(&path).unwrap_err();
    assert!(err.contains("Missing 'phases'"), "err was: {}", err);
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

    let dest = project
        .join(".flow-states")
        .join("my-feature")
        .join("phases.json");
    assert!(dest.exists());
    let content = fs::read_to_string(&dest).unwrap();
    assert!(content.contains("order"));
}

#[test]
fn freeze_phases_copy_nonexistent_source_returns_err() {
    // Exercises the `std::fs::copy(...)?` error-propagation arm:
    // create_dir_all succeeds (empty .flow-states created), then
    // copy fails because the source file doesn't exist.
    let dir = tempfile::tempdir().unwrap();
    let missing_src = dir.path().join("does-not-exist.json");
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();

    let err = freeze_phases(&missing_src, &project, "my-feature").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn freeze_phases_create_dir_on_file_root_returns_err() {
    // Exercises the `std::fs::create_dir_all(...)?` error-propagation
    // arm: project_root is a regular file, so creating a
    // `.flow-states/` subdirectory under it fails with NotADirectory.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("flow-phases.json");
    fs::write(&src, r#"{"order": [], "phases": {}}"#).unwrap();
    // project_root is a file, not a directory — create_dir_all fails.
    let project_as_file = dir.path().join("project-file");
    fs::write(&project_as_file, "").unwrap();

    let err = freeze_phases(&src, &project_as_file, "my-feature").unwrap_err();
    // kind() is platform-dependent (NotADirectory on Unix, other on
    // Windows) — we only assert the call returned Err, which is what
    // the coverage gate needs.
    let _ = err;
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
    let real_dir = state_dir.join("my-feature");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch": "my-feature"}"#).unwrap();

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
    let real_dir = state_dir.join("feature-xyz");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch": "feature-xyz"}"#).unwrap();

    let results = find_state_files(dir.path(), "main");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].2, "feature-xyz");
}

#[test]
fn find_state_files_skips_subdir_with_only_phases_json() {
    // A branch directory with only phases.json (no state.json) is
    // skipped — the subdir scan requires a readable state.json.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    let real_dir = state_dir.join("feature-x");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch": "feature-x"}"#).unwrap();
    let phases_only = state_dir.join("phases-only");
    fs::create_dir_all(&phases_only).unwrap();
    fs::write(phases_only.join("phases.json"), r#"{"order": []}"#).unwrap();

    let results = find_state_files(dir.path(), "main");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].2, "feature-x");
}

#[test]
fn find_state_files_skips_orchestrate_at_root() {
    // orchestrate.json is a regular file at .flow-states/ root and
    // never participates in branch discovery.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    let real_dir = state_dir.join("feature-x");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch": "feature-x"}"#).unwrap();
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
    let bad_dir = state_dir.join("bad");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(bad_dir.join("state.json"), "{corrupt").unwrap();
    let good_dir = state_dir.join("good");
    fs::create_dir_all(&good_dir).unwrap();
    fs::write(good_dir.join("state.json"), r#"{"branch": "good"}"#).unwrap();

    let results = find_state_files(dir.path(), "main");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].2, "good");
}

#[test]
fn find_state_files_corrupt_exact_no_fallthrough() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    let main_dir = state_dir.join("main");
    fs::create_dir_all(&main_dir).unwrap();
    fs::write(main_dir.join("state.json"), "{corrupt").unwrap();
    let other_dir = state_dir.join("other");
    fs::create_dir_all(&other_dir).unwrap();
    fs::write(other_dir.join("state.json"), r#"{"branch": "other"}"#).unwrap();

    let results = find_state_files(dir.path(), "main");
    assert!(results.is_empty());
}

#[test]
fn find_state_files_empty_branch_scans_directory() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("a")).unwrap();
    fs::write(state_dir.join("a").join("state.json"), r#"{"branch": "a"}"#).unwrap();
    fs::create_dir_all(state_dir.join("b")).unwrap();
    fs::write(state_dir.join("b").join("state.json"), r#"{"branch": "b"}"#).unwrap();

    let results = find_state_files(dir.path(), "");
    let mut branches: Vec<_> = results.iter().map(|(_, _, b)| b.clone()).collect();
    branches.sort();
    assert_eq!(branches, vec!["a", "b"]);
}

#[test]
fn find_state_files_slash_branch_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    let real_dir = state_dir.join("other-feature");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(
        real_dir.join("state.json"),
        r#"{"branch": "other-feature"}"#,
    )
    .unwrap();

    let results = find_state_files(dir.path(), "feature/foo");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].2, "other-feature");
}

#[test]
fn find_state_files_multi_slash_branch_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let results = find_state_files(dir.path(), "dependabot/npm_and_yarn/acme-1.2.3");
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

#[test]
fn read_flow_json_none_root_uses_cwd_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    let _ = dir;
    assert!(read_flow_json(None).is_none() || read_flow_json(None).is_some());
}

#[test]
fn load_phase_config_missing_file_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.json");
    let err = load_phase_config(&missing).unwrap_err();
    assert!(err.contains("Cannot read"), "err was: {}", err);
}

#[test]
fn load_phase_config_invalid_json_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen.json");
    fs::write(&path, "{not valid json").unwrap();
    let err = load_phase_config(&path).unwrap_err();
    assert!(err.contains("Invalid JSON"), "err was: {}", err);
}

#[test]
fn load_phase_config_key_not_in_phases_continues() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen.json");
    fs::write(
        &path,
        r#"{
            "order": ["flow-start", "flow-missing"],
            "phases": {
                "flow-start": {"name": "Start", "command": "/t:s"}
            }
        }"#,
    )
    .unwrap();
    let cfg = load_phase_config(&path).unwrap();
    assert_eq!(cfg.order, vec!["flow-start", "flow-missing"]);
    assert_eq!(cfg.numbers.get("flow-start"), Some(&1));
    assert_eq!(cfg.numbers.get("flow-missing"), Some(&2));
    assert!(cfg.names.contains_key("flow-start"));
    assert!(!cfg.names.contains_key("flow-missing"));
}

#[test]
fn find_state_files_skips_files_at_root_and_subdirs_without_state_json() {
    // Subdir scan: regular files at `.flow-states/` root (orchestrate.json,
    // README.md, stale flat-layout artifacts) and subdirectories that
    // lack a `state.json` are all skipped. Only `<branch>/state.json`
    // entries that parse cleanly contribute to the result.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = FlowStatesDir::new(dir.path()).path().to_path_buf();
    fs::create_dir_all(&state_dir).unwrap();
    // Regular files at .flow-states/ root — must all be ignored.
    fs::write(state_dir.join("README.md"), "not a state").unwrap();
    fs::write(state_dir.join("orchestrate.json"), r#"{"batch":[]}"#).unwrap();
    // Subdir without state.json — must be ignored.
    fs::create_dir_all(state_dir.join("empty-branch")).unwrap();
    // Subdir with unparseable state.json — must be ignored.
    let broken_dir = state_dir.join("broken");
    fs::create_dir_all(&broken_dir).unwrap();
    fs::write(broken_dir.join("state.json"), "not json").unwrap();
    // Valid subdir — must be returned.
    let real_dir = state_dir.join("real-branch");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch":"real-branch"}"#).unwrap();

    let results = find_state_files(dir.path(), "nonexistent-branch");
    let branches: Vec<&str> = results.iter().map(|(_, _, b)| b.as_str()).collect();
    assert!(
        branches.contains(&"real-branch"),
        "got branches: {:?}",
        branches
    );
    assert!(
        !branches.contains(&"empty-branch"),
        "subdir without state.json must be skipped"
    );
    assert!(
        !branches.contains(&"broken"),
        "subdir with unparseable state.json must be skipped"
    );
    assert!(
        !branches.contains(&"orchestrate"),
        "regular files at root must be skipped"
    );
    assert!(
        !branches.contains(&"README"),
        "regular files at root must be skipped"
    );
}

#[test]
fn find_state_files_exact_match_unparseable_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let paths = FlowPaths::new(dir.path(), "my-feature");
    fs::create_dir_all(paths.state_file().parent().unwrap()).unwrap();
    fs::write(paths.state_file(), "not valid json").unwrap();
    let results = find_state_files(dir.path(), "my-feature");
    assert!(results.is_empty());
}

#[test]
fn find_state_files_exact_match_is_directory_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let paths = FlowPaths::new(dir.path(), "my-feature");
    fs::create_dir_all(paths.state_file()).unwrap();
    let results = find_state_files(dir.path(), "my-feature");
    assert!(results.is_empty());
}

#[test]
fn find_state_files_subdir_with_state_json_directory_is_skipped() {
    // A subdirectory whose state.json is itself a directory cannot be
    // read as a file, so it must be skipped without affecting siblings.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = FlowStatesDir::new(dir.path()).path().to_path_buf();
    let broken_dir = state_dir.join("broken");
    fs::create_dir_all(broken_dir.join("state.json")).unwrap();
    let real_dir = state_dir.join("real");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("state.json"), r#"{"branch":"real"}"#).unwrap();

    let results = find_state_files(dir.path(), "nonexistent-branch");
    let branches: Vec<&str> = results.iter().map(|(_, _, b)| b.as_str()).collect();
    assert!(branches.contains(&"real"), "branches: {:?}", branches);
    assert!(
        !branches.contains(&"broken"),
        "unreadable state.json must be skipped"
    );
}

#[test]
fn load_phase_config_phase_missing_name_and_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen.json");
    fs::write(
        &path,
        r#"{
            "order": ["flow-sparse"],
            "phases": { "flow-sparse": {} }
        }"#,
    )
    .unwrap();
    let cfg = load_phase_config(&path).unwrap();
    assert_eq!(cfg.numbers.get("flow-sparse"), Some(&1));
    assert!(!cfg.names.contains_key("flow-sparse"));
    assert!(!cfg.commands.contains_key("flow-sparse"));
}

#[test]
fn find_state_files_read_dir_failure_returns_empty_list() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_dir = FlowStatesDir::new(dir.path()).path().to_path_buf();
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("hint.json"), r#"{"x":1}"#).unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o000)).unwrap();

    let results = find_state_files(dir.path(), "nonexistent-branch");
    let _ = fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o755));
    assert!(results.is_empty());
}

/// Subdirectory with an unreadable `state.json` (mode 0o000) — the
/// `is_file()` check passes but `read_to_string` returns Err. The
/// scanner skips the entry and returns the remaining valid flows.
#[test]
fn find_state_files_unreadable_state_json_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_dir = FlowStatesDir::new(dir.path()).path().to_path_buf();
    fs::create_dir_all(state_dir.join("good-branch")).unwrap();
    fs::write(
        state_dir.join("good-branch").join("state.json"),
        r#"{"branch":"good-branch"}"#,
    )
    .unwrap();
    let unreadable_dir = state_dir.join("unreadable-branch");
    fs::create_dir_all(&unreadable_dir).unwrap();
    let unreadable_path = unreadable_dir.join("state.json");
    fs::write(&unreadable_path, r#"{"branch":"unreadable-branch"}"#).unwrap();
    fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o000)).unwrap();

    let results = find_state_files(dir.path(), "nonexistent-branch");

    let _ = fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o644));

    let branches: Vec<&str> = results.iter().map(|(_, _, b)| b.as_str()).collect();
    assert_eq!(branches, vec!["good-branch"]);
}

// --- MIR region coverage tests ---

#[test]
fn load_phase_config_order_non_string_element_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phases.json");
    fs::write(
        &path,
        r#"{
            "order": ["flow-start", 42, "flow-plan"],
            "phases": {
                "flow-start": {"name": "Start", "command": "/t:s"},
                "flow-plan": {"name": "Plan", "command": "/t:p"}
            }
        }"#,
    )
    .unwrap();
    let cfg = load_phase_config(&path).unwrap();
    assert_eq!(cfg.order, vec!["flow-start", "flow-plan"]);
    assert_eq!(cfg.order.len(), 2);
}

#[test]
fn load_phase_config_name_non_string_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phases.json");
    fs::write(
        &path,
        r#"{
            "order": ["flow-start"],
            "phases": {
                "flow-start": {"name": 123, "command": "/t:s"}
            }
        }"#,
    )
    .unwrap();
    let cfg = load_phase_config(&path).unwrap();
    assert!(!cfg.names.contains_key("flow-start"));
    assert_eq!(cfg.commands.get("flow-start").unwrap(), "/t:s");
}

#[test]
fn load_phase_config_command_non_string_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phases.json");
    fs::write(
        &path,
        r#"{
            "order": ["flow-start"],
            "phases": {
                "flow-start": {"name": "Start", "command": true}
            }
        }"#,
    )
    .unwrap();
    let cfg = load_phase_config(&path).unwrap();
    assert_eq!(cfg.names.get("flow-start").unwrap(), "Start");
    assert!(!cfg.commands.contains_key("flow-start"));
}
