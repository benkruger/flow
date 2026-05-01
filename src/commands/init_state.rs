use std::fs;
use std::path::Path;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;
use crate::git::{default_branch_in, project_root};
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
///
/// The state's `base_branch` field is detected internally via
/// [`default_branch_in`] reading `git symbolic-ref refs/remotes/origin/HEAD`
/// from `project_root`. Falls back to `"main"` when the detection fails
/// (non-git directory, no remote, symbolic-ref unset). Downstream
/// `start-gate` and `start-workspace` read this field so a repo whose
/// integration branch is not `main` (e.g. `staging`, `develop`) still
/// has FLOW coordinate against its actual default branch.
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
    let current_time = now();
    let phases = build_initial_phases(&current_time);
    let base_branch = default_branch_in(project_root);

    let mut state = serde_json::Map::new();
    state.insert("schema_version".into(), json!(1));
    state.insert("branch".into(), json!(branch));
    state.insert("base_branch".into(), json!(base_branch));
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
    // `json!(Option<String>)` serializes Some(t) as `"t"` and None as
    // `null`, letting serde handle both arms without a match in our
    // code. This avoided exposing a `_with_tty` test seam solely to
    // drive the two branches.
    state.insert("session_tty".into(), json!(detect_tty()));
    state.insert("session_id".into(), Value::Null);
    state.insert("transcript_path".into(), Value::Null);
    state.insert("notes".into(), json!([]));
    state.insert("prompt".into(), json!(prompt));
    // `phases` is Vec<PhaseState> with only finite scalar fields.
    // `serde_json::to_value` on these never fails.
    let phases_value = serde_json::to_value(&phases).expect("PhaseState list always serializes");
    state.insert("phases".into(), phases_value);
    state.insert("phase_transitions".into(), json!([]));

    if let Some(s) = skills {
        // IndexMap<String, SkillConfig> — no float NaN, no
        // non-serializable fields. Always serializes.
        let skills_value = serde_json::to_value(s).expect("SkillConfig map always serializes");
        state.insert("skills".into(), skills_value);
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
    if let Err(e) = fs::create_dir_all(&state_dir) {
        return Err(format!("Cannot create .flow-states: {}", e));
    }
    let state_path = paths.state_file();
    // The state Map we just built contains only json!() literals plus
    // values we produced from `serde_json::to_value` above — all valid.
    // Pretty-printing a valid Value cannot fail.
    let output = serde_json::to_string_pretty(&Value::Object(state))
        .expect("state Value always pretty-prints");
    if let Err(e) = fs::write(&state_path, output) {
        return Err(format!("Cannot write state file: {}", e));
    }

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

    // `plugin_root()` walks up from the binary path looking for a
    // directory containing `flow-phases.json`. In production it always
    // resolves — the plugin ships flow-phases.json at its root. In
    // tests it resolves via the workspace layout (target/.../deps/ walks
    // up to the repo root, which contains flow-phases.json). If this
    // ever returns None in a real deployment, every subsequent FLOW
    // command fails the same way, so a fail-fast panic here is no
    // worse than the subsequent failure modes.
    let pr = plugin_root().expect("plugin_root resolves in any runnable environment");
    let phases_path = pr.join("flow-phases.json");
    if let Err(e) = freeze_phases(&phases_path, &root, &branch) {
        json_error(
            &format!("Cannot freeze phases: {}", e),
            &[("step", json!("freeze_phases"))],
        );
        std::process::exit(1);
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
