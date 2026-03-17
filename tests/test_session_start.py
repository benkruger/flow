"""Tests for hooks/session-start.sh — the SessionStart hook."""

import json
import subprocess

import pytest

from conftest import HOOKS_DIR, make_state, write_state

SCRIPT = str(HOOKS_DIR / "session-start.sh")


def _run(git_repo):
    """Run session-start.sh inside the given git repo."""
    result = subprocess.run(
        ["bash", SCRIPT],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


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
    state["feature"] = "Invoice Pdf Export"
    write_state(state_dir, "invoice-pdf-export", state)

    result = _run(git_repo)
    assert result.returncode == 0

    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow-session-context" in ctx
    assert "Invoice Pdf Export" in ctx
    assert "flow:flow-continue" in ctx


def test_single_feature_resets_session_started_at(git_repo):
    """Single feature should reset session_started_at to null in the state file."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = "2026-01-15T10:00:00Z"
    write_state(state_dir, "my-feature", state)

    _run(git_repo)

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert updated["phases"]["flow-plan"]["session_started_at"] is None


# --- Multiple features ---


def test_multiple_features_mentions_both(git_repo):
    """Multiple features → JSON mentions 'Multiple FLOW features' and both names."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["feature"] = "Feature Alpha"
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    s2["feature"] = "Feature Beta"
    write_state(state_dir, "feature-beta", s2)

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
    state["feature"] = 'Test "Feature" with \\backslash'
    write_state(state_dir, "test-special", state)

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
    state["feature"] = "Valid Feature"
    write_state(state_dir, "valid-branch", state)

    result = _run(git_repo)
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert "Valid Feature" in output["additional_context"]


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

    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "flow:flow-note" in ctx


def test_multiple_features_does_not_force_action(git_repo):
    """Multiple features context must NOT force Claude to act unprompted."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["feature"] = "Feature One"
    write_state(state_dir, "feature-one", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["feature"] = "Feature Two"
    write_state(state_dir, "feature-two", s2)

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
    s1["feature"] = "Feature One"
    write_state(state_dir, "feature-one", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["feature"] = "Feature Two"
    write_state(state_dir, "feature-two", s2)

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
    state["feature"] = "My Feature"
    state["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    write_state(state_dir, "my-feature", state)

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
    state["feature"] = "My Feature"
    state["plan_file"] = None
    write_state(state_dir, "my-feature", state)

    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Do NOT invoke flow:flow-continue" in ctx


def test_phases_json_files_are_ignored(git_repo):
    """-phases.json files (copies of flow-phases.json) must not appear as features."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    # Real state file
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["feature"] = "Real Feature"
    write_state(state_dir, "real-feature", state)

    # Ghost: a -phases.json file (copied flow-phases.json)
    (state_dir / "real-feature-phases.json").write_text(json.dumps({"phases": []}))

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
    s1["feature"] = "Plan Ready"
    s1["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    write_state(state_dir, "plan-ready", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["feature"] = "Other Work"
    write_state(state_dir, "other-work", s2)

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

    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "NEVER implement" in ctx


def test_multiple_features_includes_implementation_guardrail(git_repo):
    """Multiple features context must include implementation guardrail."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)

    s1 = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    s1["feature"] = "Feature A"
    write_state(state_dir, "feature-a", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["feature"] = "Feature B"
    write_state(state_dir, "feature-b", s2)

    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "NEVER implement" in ctx


def test_single_feature_plan_approved_includes_implementation_guardrail(git_repo):
    """Single feature with plan approved must still include implementation guardrail."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["feature"] = "My Feature"
    state["plan_file"] = "/Users/test/.claude/plans/test-plan.md"
    write_state(state_dir, "my-feature", state)

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
    state["feature"] = "Step Tracking"
    state["code_review_step"] = 2
    write_state(state_dir, "step-tracking", state)

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
    state["feature"] = "No Steps"
    write_state(state_dir, "no-steps", state)

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
    s1["feature"] = "Review Feature"
    s1["code_review_step"] = 3
    write_state(state_dir, "review-feature", s1)

    s2 = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    s2["feature"] = "Other Feature"
    write_state(state_dir, "other-feature", s2)

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
    state["feature"] = "Bad Step"
    state["code_review_step"] = "bad"
    write_state(state_dir, "bad-step", state)

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
    state["feature"] = "Empty Step"
    state["code_review_step"] = ""
    write_state(state_dir, "empty-step", state)

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
    state["feature"] = "Float Step"
    state["code_review_step"] = "2.5"
    write_state(state_dir, "float-step", state)

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
    state["feature"] = "Auto Continue"
    write_state(state_dir, "auto-continue", state)

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
    state["feature"] = "Fresh Start"
    state["phases"]["flow-start"]["started_at"] = None
    write_state(state_dir, "fresh-start", state)

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
    s1["feature"] = "Feature Alpha"
    s1["compact_summary"] = "Was debugging the payment flow."
    write_state(state_dir, "feature-alpha", s1)

    s2 = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    s2["feature"] = "Feature Beta"
    write_state(state_dir, "feature-beta", s2)

    result = _run(git_repo)
    output = json.loads(result.stdout)
    ctx = output["additional_context"]
    assert "Was debugging the payment flow." in ctx