"""Tests for lib/continue-context.py — the continue-context builder."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "continue-context.py")

# Import continue-context.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "continue_context", LIB_DIR / "continue-context.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(cwd):
    """Run continue-context.py via subprocess with no args, from cwd."""
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(cwd),
    )
    return result


# --- CLI behavior ---


def test_no_branch_returns_error(tmp_path):
    """Running outside a git repo (no branch) returns an error."""
    result = _run(tmp_path)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "branch" in data["message"]


def test_no_state_file_returns_no_state(git_repo):
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "no_state"
    assert "branch" in data


def test_corrupt_json_returns_error(state_dir, git_repo, branch):
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")
    result = _run(git_repo)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_happy_path_returns_ok_with_all_fields(state_dir, git_repo, branch):
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    state["phases"]["1"]["cumulative_seconds"] = 300
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert "panel" in data
    assert data["current_phase"] == 2
    assert data["phase_name"] == "Research"
    assert data["phase_command"] == "/flow:research"
    assert data["worktree"] == ".worktrees/test-feature"


def test_all_complete_returns_ok_with_phase_9(state_dir, git_repo, branch):
    state = make_state(
        current_phase=9,
        phase_statuses={i: "complete" for i in range(1, 10)},
    )
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["current_phase"] == 9
    assert data["phase_name"] == "Cleanup"
    assert data["phase_command"] == "/flow:cleanup"


def test_missing_worktree_still_returns_ok(state_dir, git_repo, branch):
    """Worktree field from state is passed through even if dir doesn't exist."""
    state = make_state(
        current_phase=3,
        phase_statuses={1: "complete", 2: "complete", 3: "in_progress"},
    )
    state["worktree"] = "/nonexistent/worktree/path"
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["worktree"] == "/nonexistent/worktree/path"


# --- Regression: panel identity ---


def test_panel_matches_format_status_output(state_dir, git_repo, branch):
    """The panel from continue-context must be identical to format-status."""
    # Import format-status for comparison
    fs_spec = importlib.util.spec_from_file_location(
        "format_status", LIB_DIR / "format-status.py"
    )
    fs_mod = importlib.util.module_from_spec(fs_spec)
    fs_spec.loader.exec_module(fs_mod)
    state = make_state(
        current_phase=5,
        phase_statuses={
            1: "complete", 2: "complete", 3: "complete",
            4: "complete", 5: "in_progress",
        },
    )
    state["phases"]["1"]["cumulative_seconds"] = 60
    state["phases"]["2"]["cumulative_seconds"] = 300
    state["phases"]["3"]["cumulative_seconds"] = 600
    state["phases"]["4"]["cumulative_seconds"] = 900
    state["notes"] = [{"text": "note 1"}, {"text": "note 2"}]
    write_state(state_dir, branch, state)

    # Get panel from continue-context
    cc_result = _run(git_repo)
    cc_data = json.loads(cc_result.stdout)

    # Get panel from format-status
    fs_result = subprocess.run(
        [sys.executable, str(LIB_DIR / "format-status.py")],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    fs_data = json.loads(fs_result.stdout)

    assert cc_data["panel"] == fs_data["panel"]


# --- In-process unit tests ---


def test_commands_dict_has_all_9():
    for i in range(1, 10):
        assert i in _mod.COMMANDS


def test_phase_command_matches_flow_phases_json():
    phases_json = LIB_DIR.parent / "flow-phases.json"
    phases = json.loads(phases_json.read_text())["phases"]
    for num_str, phase_data in phases.items():
        num = int(num_str)
        assert _mod.COMMANDS[num] == phase_data["command"]