//! Tests for `src/hooks/agent_run_record.rs` — the PreToolUse:Agent
//! recorder that writes a required Review/Learn agent into
//! `phases.<phase>.agents_returned` when the model launches the agent.
//!
//! Two execution modes per `.claude/rules/tests-guard-real-regressions.md`:
//!
//! - Library-level: call `record_agent_run` directly with a fixture
//!   worktree + state file and assert the set-add behavior, the guards
//!   (in_progress, required phase, subagent match), idempotency, and
//!   fail-open on corrupt/missing state.
//! - Subprocess: spawn `bin/flow hook validate-pretool` with a real
//!   Agent-shaped `PreToolUse` payload and assert the record lands —
//!   AND that no synthetic path (a Bash command, a non-required
//!   subagent) produces the record. This is the load-bearing
//!   anti-fabrication contract per `.claude/rules/verify-automation-e2e.md`
//!   (a security-sensitive authorization path, tested inline, not
//!   deferred).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tempfile::TempDir;

use flow_rs::hooks::agent_run_record::record_agent_run;

/// Build a worktree + state fixture and return
/// `(tempdir_guard, worktree_path, state_json_path)`.
///
/// Side effects on disk:
///   * `<root>/.worktrees/<branch>/.git` (a FILE) so
///     `detect_branch_from_path(worktree)` resolves `<branch>` by
///     walking up to the `.git` marker bounded by `.worktrees`.
///   * `<root>/.flow-states/<branch>/state.json` holding `state_json`
///     so `FlowPaths::state_file()` — rooted at
///     `resolve_main_root(worktree) == <root>` — resolves to a real
///     state file.
///
/// `root` is canonicalized so the string-derived `main_root` matches
/// the on-disk fixture on macOS (where `tempdir()` lives under a
/// `/var -> /private/var` symlink). Without canonicalization the
/// recorder's `resolve_main_root` would produce a path that does not
/// match where the fixture wrote the state file, and the recorder
/// would silently skip.
fn fixture(branch: &str, state_json: &str) -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");

    let worktree = root.join(".worktrees").join(branch);
    fs::create_dir_all(&worktree).expect("create worktree");
    fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/x").expect("write .git");

    let states = root.join(".flow-states").join(branch);
    fs::create_dir_all(&states).expect("create states dir");
    let state_path = states.join("state.json");
    fs::write(&state_path, state_json).expect("write state.json");

    (dir, worktree, state_path)
}

/// Build a state.json string for `phase` with the given `status`.
fn state_for(phase: &str, status: &str) -> String {
    serde_json::to_string(&json!({
        "current_phase": phase,
        "phases": { phase: { "status": status } }
    }))
    .unwrap()
}

/// Read the recorded agent names from `phases.<phase>.agents_returned`.
fn returned_agents(state_path: &Path, phase: &str) -> Vec<String> {
    let content = fs::read_to_string(state_path).expect("read state");
    let v: Value = serde_json::from_str(&content).expect("parse state");
    v["phases"][phase]["agents_returned"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e["agent"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// --- record_agent_run ---

#[test]
fn record_agent_run_records_reviewer_on_in_progress_review() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    assert_eq!(
        returned_agents(&state_path, "flow-review"),
        vec!["reviewer".to_string()],
        "a flow:reviewer launch during in-progress flow-review must record reviewer"
    );
}

#[test]
fn record_agent_run_records_learn_analyst_on_in_progress_learn() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-learn", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:learn-analyst"));
    assert_eq!(
        returned_agents(&state_path, "flow-learn"),
        vec!["learn-analyst".to_string()],
        "a flow:learn-analyst launch during in-progress flow-learn must record learn-analyst"
    );
}

#[test]
fn record_agent_run_ignores_non_required_subagent_type() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:ci-fixer"));
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "a non-required subagent (ci-fixer) must not record into a required-agent phase"
    );
}

#[test]
fn record_agent_run_ignores_subagent_for_other_phase() {
    // learn-analyst is required for flow-learn, not flow-review.
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:learn-analyst"));
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "an agent required by a different phase must not record into the current phase"
    );
}

#[test]
fn record_agent_run_ignores_when_phase_not_in_progress() {
    for status in ["pending", "complete"] {
        let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", status));
        record_agent_run(Some(&worktree), Some("flow:reviewer"));
        assert!(
            returned_agents(&state_path, "flow-review").is_empty(),
            "status={status}: a launch outside the in_progress window must not record"
        );
    }
}

#[test]
fn record_agent_run_ignores_when_no_active_flow() {
    // Worktree with a .git marker but NO state file.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).expect("create worktree");
    fs::write(worktree.join(".git"), "gitdir: x").expect("write .git");
    // Must not panic and must not create a state file.
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    assert!(
        !root
            .join(".flow-states")
            .join("feat")
            .join("state.json")
            .exists(),
        "no active flow: the recorder must not create a state file"
    );
}

#[test]
fn record_agent_run_idempotent_on_relaunch() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    assert_eq!(
        returned_agents(&state_path, "flow-review"),
        vec!["reviewer".to_string()],
        "a second launch of the same agent must not add a duplicate entry"
    );
}

#[test]
fn record_agent_run_two_distinct_agents_both_recorded() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    record_agent_run(Some(&worktree), Some("flow:adversarial"));
    let mut agents = returned_agents(&state_path, "flow-review");
    agents.sort();
    assert_eq!(
        agents,
        vec!["adversarial".to_string(), "reviewer".to_string()],
        "two distinct required agents must both record into the set"
    );
}

#[test]
fn record_agent_run_fail_open_on_corrupt_state() {
    let (_d, worktree, state_path) = fixture("feat", "{ this is not json");
    // Must not panic; corrupt state means no record.
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    let content = fs::read_to_string(&state_path).expect("read state");
    assert_eq!(
        content, "{ this is not json",
        "corrupt state must be left untouched (fail-open, no mutation)"
    );
}

#[test]
fn record_agent_run_no_cwd_is_noop() {
    // Must not panic when cwd is absent.
    record_agent_run(None, Some("flow:reviewer"));
}

#[test]
fn record_agent_run_no_subagent_type_is_noop() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), None);
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "an Agent call with no subagent_type must not record"
    );
}

#[test]
fn record_agent_run_normalizes_subagent_type_case_and_whitespace() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("  FLOW:Reviewer  "));
    assert_eq!(
        returned_agents(&state_path, "flow-review"),
        vec!["reviewer".to_string()],
        "subagent_type must be normalized (NUL-strip + trim + lowercase) before matching"
    );
}

#[test]
fn record_agent_run_concurrency_two_cwds_no_cross_record() {
    // Two independent flows on two branches, each in flow-review.
    let (_da, worktree_a, state_a) = fixture("feat-a", &state_for("flow-review", "in_progress"));
    let (_db, worktree_b, state_b) = fixture("feat-b", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree_a), Some("flow:reviewer"));
    record_agent_run(Some(&worktree_b), Some("flow:adversarial"));
    assert_eq!(
        returned_agents(&state_a, "flow-review"),
        vec!["reviewer".to_string()],
        "branch A records only its own launch"
    );
    assert_eq!(
        returned_agents(&state_b, "flow-review"),
        vec!["adversarial".to_string()],
        "branch B records only its own launch — no cross-record between cwds"
    );
}

#[test]
fn record_agent_run_heals_wrong_type_agents_returned() {
    // agents_returned present but the wrong type (a string) — object
    // guards reset it to an array before the set-add.
    let state = serde_json::to_string(&json!({
        "current_phase": "flow-review",
        "phases": { "flow-review": { "status": "in_progress", "agents_returned": "bogus" } }
    }))
    .unwrap();
    let (_d, worktree, state_path) = fixture("feat", &state);
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    assert_eq!(
        returned_agents(&state_path, "flow-review"),
        vec!["reviewer".to_string()],
        "a wrong-type agents_returned field must be reset to an array, then the agent added"
    );
}

#[test]
fn record_agent_run_fail_open_when_state_path_is_directory() {
    // state.json present but as a DIRECTORY: File::open succeeds, the
    // capped read returns Err (EISDIR), read_state_capped → None.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).expect("create worktree");
    fs::write(worktree.join(".git"), "gitdir: x").expect("write .git");
    fs::create_dir_all(root.join(".flow-states").join("feat").join("state.json"))
        .expect("create state.json as a directory");
    // Must not panic; unreadable state means no record.
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
}

#[test]
fn record_agent_run_ignores_whitespace_only_subagent_type() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    record_agent_run(Some(&worktree), Some("   "));
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "a subagent_type that normalizes to empty must not record"
    );
}

#[test]
fn record_agent_run_ignores_when_branch_unresolvable() {
    // A plain tempdir: no `.worktrees/` segment and not a git repo, so
    // detect_branch_from_path returns None.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    // Must not panic and must not create any .flow-states tree.
    record_agent_run(Some(&root), Some("flow:reviewer"));
    assert!(
        !root.join(".flow-states").exists(),
        "an unresolvable branch must produce no record"
    );
}

#[test]
fn record_agent_run_ignores_invalid_branch() {
    // A nested worktree layout where the `.git` marker sits two levels
    // below `.worktrees`, so detect_branch_from_path yields the
    // slash-containing branch `feat/sub`, which FlowPaths::try_new
    // rejects.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let nested = root.join(".worktrees").join("feat").join("sub");
    fs::create_dir_all(&nested).expect("create nested worktree");
    fs::write(nested.join(".git"), "gitdir: x").expect("write .git");
    // Must not panic; an invalid (slash-containing) branch produces no
    // record.
    record_agent_run(Some(&nested), Some("flow:reviewer"));
    assert!(
        !root.join(".flow-states").join("feat").join("sub").exists(),
        "an invalid branch must produce no record"
    );
}

#[test]
fn record_agent_run_ignores_when_current_phase_absent() {
    // State parses but carries no current_phase.
    let (_d, worktree, state_path) = fixture("feat", "{}");
    record_agent_run(Some(&worktree), Some("flow:reviewer"));
    let content = fs::read_to_string(&state_path).expect("read state");
    assert_eq!(
        content, "{}",
        "a state file with no current_phase must be left untouched"
    );
}

// --- anti-fabrication via the real PreToolUse:Agent hook ---

/// Spawn `bin/flow hook validate-pretool` with `payload` JSON at the
/// worktree cwd, returning `(exit_code, stderr)`.
fn run_pretool(payload: &Value, worktree: &Path, root: &Path) -> (i32, String) {
    let out = crate::common::spawn_hook(
        "validate-pretool",
        worktree,
        serde_json::to_string(payload).unwrap().as_bytes(),
        &[("HOME", root.to_str().unwrap())],
    );
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn record_agent_run_subprocess_real_agent_payload_records() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    // Settings file so find_settings_and_root resolves the project root.
    let claude = worktree
        .to_string_lossy()
        .replace(".worktrees/feat", ".claude");
    fs::create_dir_all(&claude).expect("create .claude");
    fs::write(format!("{claude}/settings.json"), "{}").expect("write settings");
    let root = PathBuf::from(worktree.to_string_lossy().replace(".worktrees/feat", ""));

    let payload = json!({
        "cwd": worktree.to_string_lossy(),
        "tool_input": { "subagent_type": "flow:reviewer" }
    });
    let (code, stderr) = run_pretool(&payload, &worktree, &root);
    assert_eq!(
        code, 0,
        "a flow:reviewer Agent launch must be allowed; stderr={stderr}"
    );
    assert_eq!(
        returned_agents(&state_path, "flow-review"),
        vec!["reviewer".to_string()],
        "a real PreToolUse:Agent launch must record reviewer — the launch is the evidence"
    );
}

#[test]
fn record_agent_run_subprocess_bash_command_does_not_record() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    let root = PathBuf::from(worktree.to_string_lossy().replace(".worktrees/feat", ""));
    // A Bash tool call (non-empty command) is NOT the Agent path —
    // the recorder must never fire. Use an allow-listed command.
    let payload = json!({
        "cwd": worktree.to_string_lossy(),
        "tool_input": { "command": "git status" }
    });
    let (_code, _stderr) = run_pretool(&payload, &worktree, &root);
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "a Bash tool call must never produce an agents_returned record (no synthetic path)"
    );
}

#[test]
fn record_agent_run_subprocess_non_required_subagent_does_not_record() {
    let (_d, worktree, state_path) = fixture("feat", &state_for("flow-review", "in_progress"));
    let claude = worktree
        .to_string_lossy()
        .replace(".worktrees/feat", ".claude");
    fs::create_dir_all(&claude).expect("create .claude");
    fs::write(format!("{claude}/settings.json"), "{}").expect("write settings");
    let root = PathBuf::from(worktree.to_string_lossy().replace(".worktrees/feat", ""));

    let payload = json!({
        "cwd": worktree.to_string_lossy(),
        "tool_input": { "subagent_type": "flow:ci-fixer" }
    });
    let (code, stderr) = run_pretool(&payload, &worktree, &root);
    assert_eq!(
        code, 0,
        "a flow:ci-fixer Agent launch is allowed; stderr={stderr}"
    );
    assert!(
        returned_agents(&state_path, "flow-review").is_empty(),
        "a non-required subagent launch must not record"
    );
}
