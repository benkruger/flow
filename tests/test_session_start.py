"""Tests for hooks/session-start.sh — the SessionStart hook."""

import json
import subprocess

import pytest

from conftest import HOOKS_DIR, make_orchestrate_state, make_state, write_state

SCRIPT = str(HOOKS_DIR / "session-start.sh")


def _run(git_repo):
    """Run session-start.sh inside the given git repo."""
    result = subprocess.run(
        ["bash", SCRIPT],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


def _switch(git_repo, branch_name):
    """Switch the test git repo to a named branch (for branch isolation)."""
    subprocess.run(
        ["git", "checkout", "-b", branch_name],
        cwd=str(git_repo), capture_output=True, check=True,
    )


def _detach(git_repo):
    """Detach HEAD in the test git repo (triggers fallback to scan-all)."""
    subprocess.run(
        ["git", "checkout", "--detach"],
        cwd=str(git_repo), capture_output=True, check=True,
    )


# --- No features ---


def test_no_state_directory_exits_0_silent(git_repo):
    """No .flow-states/ directory → exits 0, no stdout."""
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_empty_state_directory_exits_0_silent(git_repo):
    """Empty state directory → exits 0, no stdout."""
    (git_repo / ".flow-states").mkdir(parents=True)
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


# --- Single feature ---


def test_single_feature_returns_valid_json(git_repo):
    """Single feature → valid JSON with flow-session-context and feature name."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["branch"] = "invoice-pdf-export"
    write_state(state_dir, "invoice-pdf-export", state)

    _switch(git_repo, "invoice-pdf-export")
    result = _run(git_repo)
    assert result.returncode == 0

    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow-session-context" in ctx
    assert "Invoice Pdf Export" in ctx
    assert "flow:flow-continue" in ctx


def test_single_feature_resets_session_started_at(git_repo):
    """Single feature should reset session_started_at to null and accumulate elapsed time."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = "2026-01-15T10:00:00Z"
    state["phases"]["flow-plan"]["cumulative_seconds"] = 0
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    _run(git_repo)

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert updated["phases"]["flow-plan"]["session_started_at"] is None
    assert updated["phases"]["flow-plan"]["cumulative_seconds"] > 0


def test_reset_interrupted_preserves_existing_cumulative_seconds(git_repo):
    """Existing cumulative_seconds must be preserved when accumulating interrupted time."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = "2026-01-15T10:00:00Z"
    state["phases"]["flow-plan"]["cumulative_seconds"] = 600
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    _run(git_repo)

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert updated["phases"]["flow-plan"]["cumulative_seconds"] > 600


def test_reset_interrupted_null_session_started_at_no_change(git_repo):
    """Null session_started_at should not change cumulative_seconds."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = None
    state["phases"]["flow-plan"]["cumulative_seconds"] = 300
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    _run(git_repo)

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert updated["phases"]["flow-plan"]["cumulative_seconds"] == 300


def test_multi_feature_preserves_all_timing(git_repo):
    """All features must have their interrupted timing accumulated, not just the first."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["phases"]["flow-plan"]["session_started_at"] = "2026-01-15T10:00:00Z"
    s1["phases"]["flow-plan"]["cumulative_seconds"] = 0
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["phases"]["flow-code"]["session_started_at"] = "2026-01-15T12:00:00Z"
    s2["phases"]["flow-code"]["cumulative_seconds"] = 0
    write_state(state_dir, "feature-beta", s2)

    _detach(git_repo)
    _run(git_repo)

    updated_a = json.loads((state_dir / "feature-alpha.json").read_text())
    updated_b = json.loads((state_dir / "feature-beta.json").read_text())

    assert updated_a["phases"]["flow-plan"]["session_started_at"] is None
    assert updated_a["phases"]["flow-plan"]["cumulative_seconds"] > 0

    assert updated_b["phases"]["flow-code"]["session_started_at"] is None
    assert updated_b["phases"]["flow-code"]["cumulative_seconds"] > 0


# --- Multiple features ---


def test_multiple_features_mentions_both(git_repo):
    """Multiple features → JSON mentions 'Multiple FLOW features' and both names."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["branch"] = "feature-alpha"
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    s2["branch"] = "feature-beta"
    write_state(state_dir, "feature-beta", s2)

    _detach(git_repo)
    result = _run(git_repo)
    assert result.returncode == 0

    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Multiple FLOW features" in ctx
    assert "Feature Alpha" in ctx
    assert "Feature Beta" in ctx


# --- Edge cases ---


def test_special_characters_in_feature_name(git_repo):
    """Feature name with quotes/backslashes → output still parses as valid JSON."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    write_state(state_dir, "test-special", state)

    _switch(git_repo, "test-special")
    result = _run(git_repo)
    assert result.returncode == 0
    # Must still be valid JSON despite special chars
    output = json.loads(result.stdout)
    assert "additional_context" in output


def test_corrupt_state_file_among_valid_ones(git_repo):
    """Corrupt state file among valid ones → only valid feature appears."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    # Write a corrupt file
    (state_dir / "corrupt.json").write_text("{bad json")

    # Write a valid file
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["branch"] = "valid-branch"
    write_state(state_dir, "valid-branch", state)

    _switch(git_repo, "valid-branch")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert "Valid Branch" in output["additional_context"]


def test_all_corrupt_state_files_exits_0_silent(git_repo):
    """All state files corrupt (no valid ones) → exits 0, no meaningful output."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    (state_dir / "bad-one.json").write_text("{broken")
    (state_dir / "bad-two.json").write_text("not json at all")

    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_non_json_files_ignored(git_repo):
    """Non-.json files in state directory should be ignored."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    (state_dir / "notes.txt").write_text("not a state file")
    (state_dir / "backup.bak").write_text("also not a state file")

    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_missing_current_phase_defaults_to_phase_1(git_repo):
    """State file without current_phase should default to phase 1."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    del state["current_phase"]
    write_state(state_dir, "no-phase-field", state)

    _switch(git_repo, "no-phase-field")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert "flow-session-context" in output["additional_context"]


def test_single_feature_does_not_force_action(git_repo):
    """Single feature context must NOT force Claude to invoke flow:flow-continue."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "FIRST action" not in ctx
    assert "Invoke the flow:flow-continue skill" not in ctx


def test_single_feature_includes_note_instruction(git_repo):
    """Single feature context must include the flow:note auto-invoke instruction."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-note" in ctx


def test_multiple_features_does_not_force_action(git_repo):
    """Multiple features context must NOT force Claude to act unprompted."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    write_state(state_dir, "feature-one", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "feature-two", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "FIRST action" not in ctx
    assert "flow:flow-note" in ctx


def test_multiple_features_includes_note_instruction(git_repo):
    """Multiple features context must include the flow:note auto-invoke instruction."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    write_state(state_dir, "feature-one", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "feature-two", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-note" in ctx
    assert "corrects you" in ctx


def test_phase_2_plan_approved_instructs_auto_continue(git_repo):
    """Phase 2 with plan_file set → tells Claude to invoke flow:flow-continue
    because ExitPlanMode's 'clear context and proceed' wiped the skill context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-continue" in ctx
    assert "Do NOT invoke flow:flow-continue" not in ctx


def test_phase_2_no_plan_file_does_not_auto_continue(git_repo):
    """Phase 2 with plan_file null → normal behavior, no auto-continue."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["plan_file"] = None
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Do NOT invoke flow:flow-continue" in ctx


def test_phase_2_plan_approved_via_files_block(git_repo):
    """Phase 2 with files.plan set (new schema) → auto-continue works."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["files"]["plan"] = ".flow-states/my-feature-plan.md"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-continue" in ctx
    assert "Do NOT invoke flow:flow-continue" not in ctx


def test_phases_json_files_are_ignored(git_repo):
    """-phases.json files (copies of flow-phases.json) must not appear as features."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    # Real state file
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "real-feature"
    write_state(state_dir, "real-feature", state)

    # Ghost: a -phases.json file (copied flow-phases.json)
    (state_dir / "real-feature-phases.json").write_text(json.dumps({"phases": []}))

    _switch(git_repo, "real-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Real Feature" in ctx
    assert "None" not in ctx
    assert "Multiple" not in ctx


def test_multiple_features_plan_approved_instructs_auto_continue(git_repo):
    """Multi-feature: one at flow-plan with plan_file → auto-continue for that feature."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    s1["branch"] = "plan-ready"
    write_state(state_dir, "plan-ready", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["branch"] = "other-work"
    write_state(state_dir, "other-work", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-continue" in ctx
    assert "Plan Ready" in ctx
    assert "Do NOT invoke flow:flow-continue" not in ctx


def test_single_feature_no_plan_includes_implementation_guardrail(git_repo):
    """Single feature without plan approved must include implementation guardrail."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "NEVER implement" in ctx


def test_multiple_features_includes_implementation_guardrail(git_repo):
    """Multiple features context must include implementation guardrail."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    write_state(state_dir, "feature-a", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "feature-b", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "NEVER implement" in ctx


def test_single_feature_plan_approved_includes_implementation_guardrail(git_repo):
    """Single feature with plan approved must still include implementation guardrail."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "NEVER implement" in ctx


def test_code_review_with_step_tracking_shows_progress(git_repo):
    """Code Review at step 2 → context mentions step progress."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["code_review_step"] = 2
    write_state(state_dir, "step-tracking", state)

    _switch(git_repo, "step-tracking")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Step 2/4 done" in ctx
    assert "Security" in ctx


def test_code_review_without_step_tracking_still_works(git_repo):
    """Code Review without code_review_step → normal phase display, no step info."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["branch"] = "no-steps"
    write_state(state_dir, "no-steps", state)

    _switch(git_repo, "no-steps")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "No Steps" in ctx
    assert "Code Review" in ctx
    assert "Step" not in ctx or "Step 0" not in ctx


def test_multi_feature_code_review_step_tracking(git_repo):
    """Multi-feature with one at Code Review with step tracking → feature line includes step info."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    s1["code_review_step"] = 3
    write_state(state_dir, "review-feature", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "other-feature", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Step 3/4 done" in ctx
    assert "Code Review Plugin" in ctx


def test_code_review_bad_step_does_not_crash(git_repo):
    """Non-integer code_review_step → no crash, no step suffix in context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["code_review_step"] = "bad"
    state["branch"] = "bad-step"
    write_state(state_dir, "bad-step", state)

    _switch(git_repo, "bad-step")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Bad Step" in ctx
    assert "done" not in ctx


def test_code_review_empty_string_step_does_not_crash(git_repo):
    """Empty string code_review_step → no crash, no step suffix in context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["code_review_step"] = ""
    state["branch"] = "empty-step"
    write_state(state_dir, "empty-step", state)

    _switch(git_repo, "empty-step")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Empty Step" in ctx
    assert "done" not in ctx


def test_code_review_float_string_step_does_not_crash(git_repo):
    """Float string code_review_step → no crash, no step suffix in context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["code_review_step"] = "2.5"
    state["branch"] = "float-step"
    write_state(state_dir, "float-step", state)

    _switch(git_repo, "float-step")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Float Step" in ctx
    assert "done" not in ctx


def test_never_entered_phase_instructs_auto_continue(git_repo):
    """Phase advanced but never entered (status still pending) → auto-continue."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    # current_phase advanced to flow-code by Plan completion, but phase_enter never ran
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
    })
    write_state(state_dir, "auto-continue", state)

    _switch(git_repo, "auto-continue")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-continue" in ctx
    assert "Do NOT invoke flow:flow-continue" not in ctx


def test_phase_1_never_entered_does_not_auto_continue(git_repo):
    """Phase 1 (flow-start) with started_at None → normal behavior, no auto-continue."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["phases"]["flow-start"]["started_at"] = None
    write_state(state_dir, "fresh-start", state)

    _switch(git_repo, "fresh-start")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Do NOT invoke" in ctx


def test_output_has_both_context_fields(git_repo):
    """Output must have both additional_context and hookSpecificOutput.additionalContext."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    write_state(state_dir, "some-feature", state)

    _switch(git_repo, "some-feature")
    result = _run(git_repo)
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert "additional_context" in output
    assert "hookSpecificOutput" in output
    assert "additionalContext" in output["hookSpecificOutput"]
    assert output["additional_context"] == output["hookSpecificOutput"]["additionalContext"]


# --- Compact summary injection ---


def test_compact_summary_injected_into_context(git_repo):
    """State with compact_summary → context includes the summary in a compact-summary block."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state["compact_summary"] = "User was writing tests for webhook handler."
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "compact-summary" in ctx
    assert "User was writing tests for webhook handler." in ctx


def test_compact_summary_cleared_from_state_after_injection(git_repo):
    """After SessionStart injects compact_summary, it must be cleared from the state file."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state["compact_summary"] = "Summary to consume."
    state["compact_cwd"] = "/some/path"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    _run(git_repo)

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert "compact_summary" not in updated
    assert "compact_cwd" not in updated


def test_compact_cwd_mismatch_shows_warning(git_repo):
    """When compact_cwd does not match worktree, context includes a CWD drift warning."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state["compact_cwd"] = "/wrong/directory"
    state["worktree"] = ".worktrees/test-feature"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "/wrong/directory" in ctx
    assert ".worktrees/test-feature" in ctx


def test_compact_cwd_matches_worktree_no_warning(git_repo):
    """When compact_cwd matches worktree, no CWD drift warning appears."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state["compact_summary"] = "Summary."
    state["compact_cwd"] = ".worktrees/test-feature"
    state["worktree"] = ".worktrees/test-feature"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "compact-summary" in ctx
    assert "WARNING" not in ctx


def test_no_compact_data_no_compact_block(git_repo):
    """State without compact_summary → no compact-summary block in context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "compact-summary" not in ctx
    assert "WARNING" not in ctx


def test_multi_feature_compact_summary_injected(git_repo):
    """Multi-feature: compact_summary on one feature → included in context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    s1["compact_summary"] = "Was debugging the payment flow."
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    write_state(state_dir, "feature-beta", s2)

    _detach(git_repo)
    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Was debugging the payment flow." in ctx


# --- Branch isolation ---


def test_ignores_state_file_for_different_branch(git_repo):
    """State file for feature-alpha, session on main → silent exit (no context)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["feature"] = "Feature Alpha"
    write_state(state_dir, "feature-alpha", state)

    # git_repo is on main (default branch) — feature-alpha should not match
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_processes_only_matching_branch_state(git_repo):
    """Two state files, session on feature-alpha → context shows only Alpha."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s1["branch"] = "feature-alpha"
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    s2["branch"] = "feature-beta"
    write_state(state_dir, "feature-beta", s2)

    _switch(git_repo, "feature-alpha")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Feature Alpha" in ctx
    assert "Feature Beta" not in ctx
    assert "Multiple" not in ctx


def test_detached_head_single_file_fallback(git_repo):
    """Detached HEAD with single state file → fallback to old behavior (processes it)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "solo-feature"
    write_state(state_dir, "solo-feature", state)

    _detach(git_repo)
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Solo Feature" in ctx


def test_detached_head_multiple_files_fallback(git_repo):
    """Detached HEAD with two state files → fallback to old multi-feature behavior."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s1["branch"] = "feature-one"
    write_state(state_dir, "feature-one", s1)

    s2 = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    s2["branch"] = "feature-two"
    write_state(state_dir, "feature-two", s2)

    _detach(git_repo)
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Multiple FLOW features" in ctx
    assert "Feature One" in ctx
    assert "Feature Two" in ctx


# --- Tab title does not pollute stdout ---


def test_tab_title_does_not_appear_in_stdout(git_repo):
    """Tab title escape sequence must not appear in stdout (it goes to /dev/tty)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "tab-title-test"
    write_state(state_dir, "tab-title-test", state)

    _switch(git_repo, "tab-title-test")
    result = _run(git_repo)
    assert result.returncode == 0

    # stdout must be valid JSON — no escape sequence bytes mixed in
    output = json.loads(result.stdout)
    assert "additional_context" in output

    # The OSC title escape sequence must not appear in stdout
    assert "\033]0;" not in result.stdout
    assert "\007" not in result.stdout


def test_tab_title_with_issue_numbers_does_not_appear_in_stdout(git_repo):
    """Tab title with issue prefix from prompt must not appear in stdout."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "tab-issue-test"
    state["prompt"] = "work on issue #342"
    write_state(state_dir, "tab-issue-test", state)

    _switch(git_repo, "tab-issue-test")
    result = _run(git_repo)
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert "additional_context" in output

    assert "\033]0;" not in result.stdout
    assert "\007" not in result.stdout


# --- Orchestrator state detection ---


def _make_orch_state(**kwargs):
    """Shorthand for make_orchestrate_state in session-start tests."""
    return make_orchestrate_state(**kwargs)


def test_orchestrate_in_progress_injects_resume(git_repo):
    """In-progress orchestrate.json → context mentions orchestration and resume."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    orch_state = _make_orch_state(
        current_index=1,
        queue=[
            {"issue_number": 42, "title": "Add PDF export", "status": "completed",
             "outcome": "completed"},
            {"issue_number": 43, "title": "Fix login timeout", "status": "in_progress",
             "outcome": None},
            {"issue_number": 44, "title": "Refactor auth", "status": "pending",
             "outcome": None},
        ],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))

    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "orchestrat" in ctx.lower()
    assert "#43" in ctx
    assert "flow-orchestrate" in ctx.lower()


def test_orchestrate_completed_injects_report(git_repo):
    """Completed orchestrate.json → context includes morning report content."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    orch_state = _make_orch_state(
        completed_at="2026-03-21T06:00:00-07:00",
        queue=[
            {"issue_number": 42, "title": "Add PDF export", "status": "completed",
             "outcome": "completed"},
        ],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))

    summary_content = "# FLOW Orchestration Report\n\nCompleted: 1, Failed: 0"
    (state_dir / "orchestrate-summary.md").write_text(summary_content)

    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "orchestrat" in ctx.lower()
    assert "Orchestration Report" in ctx


def test_orchestrate_completed_cleans_up(git_repo):
    """After detecting completed orchestration, files are deleted."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    orch_state = _make_orch_state(
        completed_at="2026-03-21T06:00:00-07:00",
        queue=[],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))
    (state_dir / "orchestrate-summary.md").write_text("# Report")
    (state_dir / "orchestrate.log").write_text("log line")
    (state_dir / "orchestrate-queue.json").write_text('[{"issue_number": 42}]')

    _run(git_repo)

    assert not (state_dir / "orchestrate.json").exists()
    assert not (state_dir / "orchestrate-summary.md").exists()
    assert not (state_dir / "orchestrate.log").exists()
    assert not (state_dir / "orchestrate-queue.json").exists()


def test_orchestrate_coexists_with_feature(git_repo):
    """Orchestrate state alongside branch-scoped feature → both contexts injected."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    # Orchestrate state (in progress)
    orch_state = _make_orch_state(
        current_index=0,
        queue=[
            {"issue_number": 42, "title": "Add PDF export", "status": "in_progress",
             "outcome": None},
        ],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))

    # Feature state
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "some-feature"
    write_state(state_dir, "some-feature", state)

    _switch(git_repo, "some-feature")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "orchestrat" in ctx.lower()
    assert "Some Feature" in ctx


def test_orchestrate_missing_summary(git_repo):
    """Completed orchestration without summary file → graceful, still cleans up."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    orch_state = _make_orch_state(
        completed_at="2026-03-21T06:00:00-07:00",
        queue=[],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))
    # No summary file — should not crash

    result = _run(git_repo)
    assert result.returncode == 0
    # orchestrate.json should still be cleaned up
    assert not (state_dir / "orchestrate.json").exists()


def test_orchestrate_all_processed_no_resume(git_repo):
    """All queue items have outcomes but completed_at is None → no resume injection.

    When the orchestrator has processed all items but hasn't called --complete
    yet, the session-start hook should not inject resume context into other
    sessions. The orchestrator will self-invoke --continue-step to reach Done.
    """
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    orch_state = _make_orch_state(
        current_index=2,
        queue=[
            {"issue_number": 42, "title": "Add PDF export", "status": "completed",
             "outcome": "completed"},
            {"issue_number": 43, "title": "Fix login timeout", "status": "failed",
             "outcome": "failed"},
            {"issue_number": 44, "title": "Refactor auth", "status": "completed",
             "outcome": "completed"},
        ],
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch_state))

    result = _run(git_repo)
    assert result.returncode == 0
    # Hook should exit silently — no context injected for other sessions
    assert result.stdout.strip() == ""


def test_no_orchestrate_file_existing_behavior(git_repo):
    """No orchestrate.json → existing behavior unchanged, no orchestration context."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["branch"] = "normal-feature"
    write_state(state_dir, "normal-feature", state)

    _switch(git_repo, "normal-feature")
    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Normal Feature" in ctx
    assert "orchestrat" not in ctx.lower()
