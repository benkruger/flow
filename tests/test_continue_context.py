"""Tests for lib/continue-context.py — the continue-context builder."""

import importlib.util
import json
import sys

import pytest

from conftest import LIB_DIR, PHASE_ORDER, make_state, write_state
from flow_utils import read_version

# Import continue-context.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "continue_context", LIB_DIR / "continue-context.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- CLI behavior (in-process main()) ---


def test_no_branch_returns_error(tmp_path, monkeypatch, capsys):
    """Running outside a git repo (no branch) returns an error."""
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "branch" in data["message"]


def test_no_state_file_returns_no_state(git_repo, monkeypatch, capsys):
    """Running with no state file returns no_state."""
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "no_state"
    assert "branch" in data


def test_corrupt_json_returns_no_state(state_dir, git_repo, branch, monkeypatch, capsys):
    """Corrupt state file for current branch is treated as no state."""
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "no_state"


def test_happy_path_returns_ok_with_all_fields(state_dir, git_repo, branch, monkeypatch, capsys):
    """Happy path returns ok with all expected fields."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 300
    write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert "panel" in data
    assert data["current_phase"] == "flow-plan"
    assert data["phase_name"] == "Plan"
    assert data["phase_command"] == "/flow:flow-plan"
    assert data["worktree"] == f".worktrees/{branch}"


def test_all_complete_returns_ok_with_phase_6():
    """Phase 6 maps to Complete with /flow:flow-complete command."""
    assert _mod.PHASE_NAMES["flow-complete"] == "Complete"
    assert _mod.COMMANDS["flow-complete"] == "/flow:flow-complete"


def test_worktree_derived_from_branch(state_dir, git_repo, branch, monkeypatch, capsys):
    """Worktree field is derived from the matched branch name."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["worktree"] == f".worktrees/{branch}"


# --- Regression: panel identity ---


def test_panel_matches_format_status_output():
    """Panel from continue-context uses the same format_panel() as format-status."""
    # continue-context.py does `format_panel = _fs_mod.format_panel` — verify identity
    assert _mod.format_panel is _mod._fs_mod.format_panel

    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 60
    state["phases"]["flow-plan"]["cumulative_seconds"] = 300
    state["phases"]["flow-code"]["cumulative_seconds"] = 600
    state["notes"] = [{"text": "note 1"}, {"text": "note 2"}]

    version = read_version()
    panel = _mod.format_panel(state, version)
    assert isinstance(panel, str) and len(panel) > 0
    assert "Phase 4" in panel
    assert "Notes   : 2" in panel


# --- In-process unit tests ---


def test_commands_dict_has_all_6():
    for key in PHASE_ORDER:
        assert key in _mod.COMMANDS


def test_phase_command_matches_flow_phases_json():
    phases_json = LIB_DIR.parent / "flow-phases.json"
    phases = json.loads(phases_json.read_text())["phases"]
    for key, phase_data in phases.items():
        assert _mod.COMMANDS[key] == phase_data["command"]


# --- Fallback behavior (wrong branch) ---


def test_wrong_branch_single_feature_returns_ok(tmp_path):
    """find_state_files() falls back to the only existing state file."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["branch"] = "feature-xyz"
    (state_dir / "feature-xyz.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "some-other-branch")

    assert len(results) == 1
    _, matched_state, matched_branch = results[0]
    assert matched_branch == "feature-xyz"
    assert matched_state["current_phase"] == "flow-code"


def test_wrong_branch_multiple_features_returns_multiple(state_dir, git_repo, branch, monkeypatch, capsys):
    """When on wrong branch with multiple state files, returns multiple_features."""
    for name in ["feature-a", "feature-b"]:
        state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
        state["branch"] = name
        write_state(state_dir, name, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "multiple_features"
    assert len(data["features"]) == 2


def test_ok_response_includes_branch_field(tmp_path):
    """find_state_files() returns the matched branch name in the result tuple."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    (state_dir / "test-feature.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "test-feature")

    assert len(results) == 1
    _, _, matched_branch = results[0]
    assert matched_branch == "test-feature"


# --- --branch flag (in-process main()) ---


def test_cli_branch_flag_uses_specified_state_file(state_dir, git_repo, monkeypatch, capsys):
    """--branch flag finds the state file for a different branch."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    write_state(state_dir, "other-feature", state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["continue-context", "--branch", "other-feature"])
    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["current_phase"] == "flow-code"
    assert data["branch"] == "other-feature"
