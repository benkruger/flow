use std::fs;
use std::path::Path;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::label_issues::LABEL;
use crate::output::{json_error, json_ok};
use crate::phase_config::{auto_skills, build_initial_phases, freeze_phases, read_flow_json};
use crate::state::SkillConfig;
use crate::utils::{
    branch_name, check_duplicate_issue, detect_tty, extract_issue_numbers, fetch_issue_info, now,
    plugin_root, read_prompt_file,
};

/// Create the initial FLOW state file with null PR fields.
///
/// Builds the state as a `serde_json::Value` so the top-level key
/// order is fixed and predictable across runs — tests and hand-edited
/// state files rely on the deterministic order. Writes to
/// `.flow-states/<branch>.json`.
///
/// `commit_format` — optional commit message format (`"full"` or `"title-only"`)
/// extracted from `.flow.json` during prime. Written to state file when present;
/// the commit skill reads it at runtime.
///
/// `relative_cwd` — relative path inside the worktree where the agent
/// should operate. Empty string means worktree root. Captured by
/// `start_init` from the user's cwd at flow-start time so mono-repo
/// flows started inside a subdirectory land back in the same subdirectory
/// after worktree creation.
#[allow(clippy::too_many_arguments)]
pub fn create_state(
    project_root: &Path,
    branch: &str,
    skills: Option<&IndexMap<String, SkillConfig>>,
    prompt: &str,
    commit_format: Option<&str>,
    start_step: Option<i64>,
    start_steps_total: Option<i64>,
    relative_cwd: &str,
) -> Result<(), String> {
    create_state_with_tty(
        project_root,
        branch,
        skills,
        prompt,
        commit_format,
        start_step,
        start_steps_total,
        relative_cwd,
        detect_tty(),
    )
}

/// Test seam for `create_state` that accepts the computed session-tty
/// value as a parameter. The public wrapper above calls `detect_tty()`
/// to produce the value; this inner form lets unit tests pass
/// `Some("/dev/ttys0")` or `None` directly so both arms of the
/// `match tty { Some(t) => json!(t), None => Value::Null }` branch
/// are exercised without requiring a real PTY. No production caller
/// uses this function directly.
#[allow(clippy::too_many_arguments)]
fn create_state_with_tty(
    project_root: &Path,
    branch: &str,
    skills: Option<&IndexMap<String, SkillConfig>>,
    prompt: &str,
    commit_format: Option<&str>,
    start_step: Option<i64>,
    start_steps_total: Option<i64>,
    relative_cwd: &str,
    tty: Option<String>,
) -> Result<(), String> {
    let current_time = now();
    let phases = build_initial_phases(&current_time);

    let mut state = serde_json::Map::new();
    state.insert("schema_version".into(), json!(1));
    state.insert("branch".into(), json!(branch));
    state.insert("relative_cwd".into(), json!(relative_cwd));
    state.insert("repo".into(), Value::Null);
    state.insert("pr_number".into(), Value::Null);
    state.insert("pr_url".into(), Value::Null);
    state.insert("started_at".into(), json!(current_time));
    state.insert("current_phase".into(), json!("flow-start"));
    state.insert(
        "files".into(),
        json!({
            "plan": null,
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch),
        }),
    );
    state.insert(
        "session_tty".into(),
        match tty {
            Some(t) => json!(t),
            None => Value::Null,
        },
    );
    state.insert("session_id".into(), Value::Null);
    state.insert("transcript_path".into(), Value::Null);
    state.insert("notes".into(), json!([]));
    state.insert("prompt".into(), json!(prompt));
    state.insert(
        "phases".into(),
        serde_json::to_value(&phases).map_err(|e| e.to_string())?,
    );
    state.insert("phase_transitions".into(), json!([]));

    if let Some(s) = skills {
        state.insert(
            "skills".into(),
            serde_json::to_value(s).map_err(|e| e.to_string())?,
        );
    }
    if let Some(f) = commit_format {
        state.insert("commit_format".into(), json!(f));
    }
    if let Some(step) = start_step {
        state.insert("start_step".into(), json!(step));
    }
    if let Some(total) = start_steps_total {
        state.insert("start_steps_total".into(), json!(total));
    }

    let paths = FlowPaths::new(project_root, branch);
    let state_dir = paths.flow_states_dir();
    fs::create_dir_all(&state_dir).map_err(|e| format!("Cannot create .flow-states: {}", e))?;
    let state_path = paths.state_file();
    let output = serde_json::to_string_pretty(&Value::Object(state))
        .map_err(|e| format!("JSON serialize error: {}", e))?;
    fs::write(&state_path, output).map_err(|e| format!("Cannot write state file: {}", e))?;

    Ok(())
}

/// CLI entry point for `flow-rs init-state`.
///
/// When `branch_override` is `Some`, skip issue extraction, label guard,
/// duplicate check, and branch derivation — use the provided branch directly.
/// This is the normal path when called from `start-init`, which already
/// derived the canonical branch before acquiring the lock.
///
/// `relative_cwd` — relative path inside the project root captured by
/// `start_init` at flow-start time. Persisted into the state file so
/// downstream commands (cwd_scope guard, start_workspace cd target) can
/// route the agent back to the same subdirectory after the worktree is
/// created. Defaults to empty string.
#[allow(clippy::too_many_arguments)]
pub fn run(
    feature_name: &str,
    prompt_file: Option<&str>,
    auto: bool,
    start_step: Option<i64>,
    start_steps_total: Option<i64>,
    branch_override: Option<&str>,
    relative_cwd: &str,
) {
    let root = project_root();

    let flow_json = match read_flow_json(Some(&root)) {
        Some(data) => data,
        None => {
            json_error("Could not read .flow.json", &[]);
            std::process::exit(1);
        }
    };

    let skills = if auto {
        Some(auto_skills())
    } else {
        flow_json
            .get("skills")
            .and_then(|v| serde_json::from_value::<IndexMap<String, SkillConfig>>(v.clone()).ok())
    };

    // Read prompt first — needed for issue number extraction
    let prompt = if let Some(pf) = prompt_file {
        match read_prompt_file(std::path::Path::new(pf)) {
            Ok(content) => content,
            Err(_) => {
                json_error(
                    &format!("Could not read prompt file: {}", pf),
                    &[("step", json!("prompt_file"))],
                );
                std::process::exit(1);
            }
        }
    } else {
        feature_name.to_string()
    };

    // When --branch is provided (from start-init), skip all derivation — the
    // canonical branch was already derived pre-lock. When absent (direct CLI
    // usage), derive as before for backwards compatibility.
    let branch = if let Some(b) = branch_override {
        b.to_string()
    } else {
        // Issue-aware branch naming: fetch title AND labels in one call (issue #887).
        let issue_numbers = extract_issue_numbers(&prompt);
        let derived = if !issue_numbers.is_empty() {
            match fetch_issue_info(issue_numbers[0]) {
                Some(info) => {
                    if info.labels.iter().any(|l| l == LABEL) {
                        json_error(
                            &format!(
                                "Issue #{} already carries the '{}' label — another flow is in progress. Resume the existing flow in its worktree, or reference a different issue.",
                                issue_numbers[0], LABEL
                            ),
                            &[("step", json!("flow_in_progress_label"))],
                        );
                        std::process::exit(1);
                    }
                    branch_name(&info.title)
                }
                None => {
                    json_error(
                        &format!("Could not fetch title for issue #{}", issue_numbers[0]),
                        &[("step", json!("fetch_issue_title"))],
                    );
                    std::process::exit(1);
                }
            }
        } else {
            branch_name(feature_name)
        };

        // Duplicate issue guard: check before creating state file
        if !issue_numbers.is_empty() {
            if let Some(dup) = check_duplicate_issue(&root, &issue_numbers, &derived) {
                json_error(
                    &format!(
                        "Issue already has an active flow on branch '{}' (phase: {}, PR: {}). Resume the existing flow instead.",
                        dup.branch, dup.phase, dup.pr_url
                    ),
                    &[("step", json!("duplicate_issue"))],
                );
                std::process::exit(1);
            }
        }

        derived
    };

    let commit_format_owned = flow_json
        .get("commit_format")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let commit_format = commit_format_owned.as_deref();

    if let Err(e) = create_state(
        &root,
        &branch,
        skills.as_ref(),
        &prompt,
        commit_format,
        start_step,
        start_steps_total,
        relative_cwd,
    ) {
        json_error(&e, &[("step", json!("create_state"))]);
        std::process::exit(1);
    }

    let _ = append_log(
        &root,
        &branch,
        &format!("[Phase 1] create .flow-states/{}.json (exit 0)", branch),
    );

    match plugin_root() {
        Some(pr) => {
            let phases_path = pr.join("flow-phases.json");
            if let Err(e) = freeze_phases(&phases_path, &root, &branch) {
                json_error(
                    &format!("Cannot freeze phases: {}", e),
                    &[("step", json!("freeze_phases"))],
                );
                std::process::exit(1);
            }
        }
        None => {
            json_error(
                "Cannot find flow-phases.json",
                &[("step", json!("freeze_phases"))],
            );
            std::process::exit(1);
        }
    }

    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase 1] freeze .flow-states/{}-phases.json (exit 0)",
            branch
        ),
    );

    json_ok(&[
        ("branch", json!(branch)),
        ("state_file", json!(format!(".flow-states/{}.json", branch))),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read_state(root: &Path, branch: &str) -> Value {
        let path = root.join(".flow-states").join(format!("{}.json", branch));
        let content = fs::read_to_string(&path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    // --- Happy path ---

    #[test]
    fn create_state_writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        create_state(
            dir.path(),
            "test-feature",
            None,
            "test prompt",
            None,
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "test-feature");
        assert_eq!(state["schema_version"], 1);
        assert_eq!(state["branch"], "test-feature");
        assert_eq!(state["current_phase"], "flow-start");
    }

    /// Covers the `Some(t) => json!(t)` arm on line 94 of
    /// `create_state_with_tty`: subprocess tests spawn `flow-rs` via
    /// `.output()` which does not allocate a PTY, so `detect_tty()`
    /// returns None in every existing test. The inner seam accepts an
    /// explicit `Option<String>` so this unit test exercises the
    /// `Some` branch by passing a synthesized tty string.
    #[test]
    fn create_state_with_tty_some_writes_tty_to_state() {
        let dir = tempfile::tempdir().unwrap();
        create_state_with_tty(
            dir.path(),
            "tty-some",
            None,
            "prompt",
            None,
            None,
            None,
            "",
            Some("/dev/ttys999".to_string()),
        )
        .unwrap();
        let state = read_state(dir.path(), "tty-some");
        assert_eq!(state["session_tty"], "/dev/ttys999");
    }

    #[test]
    fn create_state_with_tty_none_writes_null() {
        let dir = tempfile::tempdir().unwrap();
        create_state_with_tty(
            dir.path(),
            "tty-none",
            None,
            "prompt",
            None,
            None,
            None,
            "",
            None,
        )
        .unwrap();
        let state = read_state(dir.path(), "tty-none");
        assert!(state["session_tty"].is_null());
    }

    // --- Null PR fields ---

    #[test]
    fn create_state_null_pr_fields() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "pr-null-test", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "pr-null-test");
        assert!(state["pr_number"].is_null());
        assert!(state["pr_url"].is_null());
        assert!(state["repo"].is_null());
    }

    // --- Phase structure ---

    #[test]
    fn create_state_has_six_phases() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "six-phases", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "six-phases");
        let phases = state["phases"].as_object().unwrap();
        assert_eq!(phases.len(), 6);
        assert_eq!(phases["flow-start"]["name"], "Start");
        assert_eq!(phases["flow-plan"]["name"], "Plan");
        assert_eq!(phases["flow-code"]["name"], "Code");
        assert_eq!(phases["flow-code-review"]["name"], "Code Review");
        assert_eq!(phases["flow-learn"]["name"], "Learn");
        assert_eq!(phases["flow-complete"]["name"], "Complete");
    }

    #[test]
    fn create_state_first_phase_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "phase-status", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "phase-status");
        let start = &state["phases"]["flow-start"];
        assert_eq!(start["status"], "in_progress");
        assert!(start["started_at"].is_string());
        assert!(start["session_started_at"].is_string());
        assert_eq!(start["visit_count"], 1);
    }

    #[test]
    fn create_state_other_phases_pending() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "pending-test", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "pending-test");
        for key in [
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ] {
            let phase = &state["phases"][key];
            assert_eq!(
                phase["status"], "pending",
                "Phase {} should be pending",
                key
            );
            assert!(
                phase["started_at"].is_null(),
                "Phase {} started_at should be null",
                key
            );
            assert_eq!(
                phase["visit_count"], 0,
                "Phase {} visit_count should be 0",
                key
            );
        }
    }

    // --- Skills ---

    #[test]
    fn create_state_skills_included() {
        let dir = tempfile::tempdir().unwrap();
        let mut skills = IndexMap::new();
        let mut start_config = IndexMap::new();
        start_config.insert("continue".to_string(), "manual".to_string());
        skills.insert(
            "flow-start".to_string(),
            SkillConfig::Detailed(start_config),
        );
        create_state(
            dir.path(),
            "skills-test",
            Some(&skills),
            "",
            None,
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "skills-test");
        assert_eq!(state["skills"]["flow-start"]["continue"], "manual");
    }

    #[test]
    fn create_state_skills_omitted_when_none() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "no-skills", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "no-skills");
        assert!(state.get("skills").is_none());
    }

    #[test]
    fn create_state_auto_skills_values() {
        let dir = tempfile::tempdir().unwrap();
        let skills = auto_skills();
        create_state(
            dir.path(),
            "auto-test",
            Some(&skills),
            "",
            None,
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "auto-test");
        assert_eq!(state["skills"]["flow-start"]["continue"], "auto");
        assert_eq!(state["skills"]["flow-code"]["commit"], "auto");
        assert_eq!(state["skills"]["flow-code"]["continue"], "auto");
        assert_eq!(state["skills"]["flow-code-review"]["commit"], "auto");
        assert_eq!(state["skills"]["flow-abort"], "auto");
    }

    // --- Prompt ---

    #[test]
    fn create_state_prompt_stored() {
        let dir = tempfile::tempdir().unwrap();
        create_state(
            dir.path(),
            "prompt-test",
            None,
            "fix issue #42 with special chars: && | ;",
            None,
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "prompt-test");
        assert_eq!(state["prompt"], "fix issue #42 with special chars: && | ;");
    }

    // --- Start step tracking ---

    #[test]
    fn create_state_start_step_fields() {
        let dir = tempfile::tempdir().unwrap();
        create_state(
            dir.path(),
            "step-test",
            None,
            "",
            None,
            Some(3),
            Some(11),
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "step-test");
        assert_eq!(state["start_step"], 3);
        assert_eq!(state["start_steps_total"], 11);
    }

    #[test]
    fn create_state_start_step_absent_when_none() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "no-step", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "no-step");
        assert!(state.get("start_step").is_none());
        assert!(state.get("start_steps_total").is_none());
    }

    // --- Files block ---

    #[test]
    fn create_state_files_block() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "files-test", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "files-test");
        let files = &state["files"];
        assert!(files["plan"].is_null());
        assert!(files["dag"].is_null());
        assert_eq!(files["log"], ".flow-states/files-test.log");
        assert_eq!(files["state"], ".flow-states/files-test.json");
    }

    // --- Top-level fields ---

    #[test]
    fn create_state_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        create_state(
            dir.path(),
            "fields-test",
            None,
            "my prompt",
            None,
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "fields-test");
        assert_eq!(state["schema_version"], 1);
        assert_eq!(state["branch"], "fields-test");
        assert_eq!(state["current_phase"], "flow-start");
        assert_eq!(state["notes"], json!([]));
        assert_eq!(state["phase_transitions"], json!([]));
        assert!(state["session_tty"].is_null() || state["session_tty"].is_string());
        assert!(state["session_id"].is_null());
        assert!(state["transcript_path"].is_null());
        assert!(state["started_at"].is_string());
    }

    // --- JSON key order ---

    #[test]
    fn create_state_key_order_matches_python() {
        let dir = tempfile::tempdir().unwrap();
        let skills = auto_skills();
        create_state(
            dir.path(),
            "order-test",
            Some(&skills),
            "test",
            Some("full"),
            Some(3),
            Some(11),
            "",
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join(".flow-states/order-test.json")).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        let keys: Vec<&String> = state.as_object().unwrap().keys().collect();
        let expected = vec![
            "schema_version",
            "branch",
            "relative_cwd",
            "repo",
            "pr_number",
            "pr_url",
            "started_at",
            "current_phase",
            "files",
            "session_tty",
            "session_id",
            "transcript_path",
            "notes",
            "prompt",
            "phases",
            "phase_transitions",
            "skills",
            "commit_format",
            "start_step",
            "start_steps_total",
        ];
        assert_eq!(
            keys, expected,
            "Key order must remain stable across serialization runs"
        );
    }

    // --- Directory creation ---

    #[test]
    fn create_state_creates_flow_states_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!dir.path().join(".flow-states").exists());
        create_state(dir.path(), "dir-test", None, "", None, None, None, "").unwrap();
        assert!(dir.path().join(".flow-states").is_dir());
        assert!(dir.path().join(".flow-states/dir-test.json").exists());
    }

    // --- Commit format ---

    #[test]
    fn create_state_commit_format_propagation() {
        let dir = tempfile::tempdir().unwrap();
        create_state(
            dir.path(),
            "cf-test",
            None,
            "",
            Some("title-only"),
            None,
            None,
            "",
        )
        .unwrap();
        let state = read_state(dir.path(), "cf-test");
        assert_eq!(state["commit_format"], "title-only");
    }

    #[test]
    fn create_state_commit_format_absent_when_none() {
        let dir = tempfile::tempdir().unwrap();
        create_state(dir.path(), "cf-none", None, "", None, None, None, "").unwrap();
        let state = read_state(dir.path(), "cf-none");
        assert!(state.get("commit_format").is_none());
    }
}
