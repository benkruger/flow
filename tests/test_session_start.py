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