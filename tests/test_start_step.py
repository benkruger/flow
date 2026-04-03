"""Tests for start-step — Start phase step counter updates (Rust implementation)."""

import json
import pathlib
import subprocess

import pytest
from conftest import BIN_DIR, make_flow_json, make_state, write_state

BIN_FLOW = str(BIN_DIR / "flow")


@pytest.fixture
def _state_with_flow_json(target_project, state_dir, branch):
    """Create a state file and .flow.json for start-step tests."""
    state = make_state()
    write_state(state_dir, branch, state)
    make_flow_json(target_project)
    return target_project, state_dir, branch


def _run(cwd, *args):
    """Run start-step via bin/flow."""
    result = subprocess.run(
        [BIN_FLOW, "start-step", *args],
        capture_output=True,
        text=True,
        cwd=str(cwd),
    )
    return result


# --- Standalone mode ---


def test_standalone_updates_state(target_project, state_dir, branch):
    """Standalone mode updates start_step and returns ok JSON."""
    state = make_state()
    write_state(state_dir, branch, state)
    make_flow_json(target_project)
    result = _run(target_project, "--step", "5", "--branch", branch)
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["step"] == 5
    updated = json.loads((state_dir / f"{branch}.json").read_text())
    assert updated["start_step"] == 5


def test_standalone_missing_state_file(target_project, state_dir, branch):
    """Standalone mode returns skipped when state file does not exist."""
    make_flow_json(target_project)
    result = _run(target_project, "--step", "5", "--branch", branch)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "skipped"
    assert "no state file" in data["reason"]


# --- Wrapping mode (end-to-end subprocess) ---


def test_wrapping_end_to_end(target_project, state_dir, branch):
    """End-to-end: start-step wraps set-timestamp, both updates visible in state file."""
    state = make_state()
    write_state(state_dir, branch, state)
    make_flow_json(target_project)
    result = subprocess.run(
        [
            BIN_FLOW,
            "start-step",
            "--step",
            "6",
            "--branch",
            branch,
            "--",
            "set-timestamp",
            "--branch",
            branch,
            "--set",
            "code_task=0",
        ],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    assert result.returncode == 0, result.stderr
    # Wrapped command output is set-timestamp JSON
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    # Both updates landed in state file
    updated = json.loads((state_dir / f"{branch}.json").read_text())
    assert updated["start_step"] == 6
    assert updated["code_task"] == 0


# --- CLI routing ---


def test_cli_via_bin_flow(target_project, state_dir, branch):
    """bin/flow start-step routes correctly."""
    state = make_state()
    write_state(state_dir, branch, state)
    make_flow_json(target_project)
    result = subprocess.run(
        [BIN_FLOW, "start-step", "--step", "3", "--branch", branch],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["step"] == 3


# --- Argument validation ---


def test_missing_step_arg(target_project):
    """Missing --step argument exits with error."""
    result = _run(target_project, "--branch", "test")
    assert result.returncode == 2  # clap exits with 2 for usage errors


def test_missing_branch_arg(target_project):
    """Missing --branch argument exits with error."""
    result = _run(target_project, "--step", "5")
    assert result.returncode == 2  # clap exits with 2 for usage errors


# --- tombstone tests ---


def test_no_python_start_step():
    """Tombstone: start-step.py removed in PR #809, ported to Rust. Must not return."""
    source = pathlib.Path(__file__).resolve().parent.parent / "lib" / "start-step.py"
    assert not source.exists(), "lib/start-step.py was removed — start-step is now in Rust"
