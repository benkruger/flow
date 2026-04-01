"""Tests for lib/start-step.py — Start phase step counter updates."""

import json
import subprocess
import sys

from conftest import BIN_DIR, LIB_DIR, import_lib, make_flow_json, make_state, write_state

SCRIPT = str(LIB_DIR / "start-step.py")
mod = import_lib("start-step.py")


def _run(cwd, *args):
    """Run start-step.py via subprocess."""
    result = subprocess.run(
        [sys.executable, SCRIPT, *args],
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
    result = _run(target_project, "--step", "5", "--branch", branch)
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["step"] == 5
    updated = json.loads((state_dir / f"{branch}.json").read_text())
    assert updated["start_step"] == 5


def test_standalone_missing_state_file(target_project, state_dir, branch):
    """Standalone mode returns skipped when state file does not exist."""
    result = _run(target_project, "--step", "5", "--branch", branch)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "skipped"
    assert "no state file" in data["reason"]


def test_standalone_corrupt_state_file(target_project, state_dir, branch):
    """Standalone mode returns skipped when state file is corrupt JSON."""
    (state_dir / f"{branch}.json").write_text("not valid json{{{")
    result = _run(target_project, "--step", "5", "--branch", branch)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "skipped"


# --- Wrapping mode (in-process with mocked execvp) ---


def test_wrapping_updates_state_and_execs(target_project, state_dir, branch, monkeypatch, capsys):
    """Wrapping mode updates state then execs the subcommand."""
    state = make_state()
    write_state(state_dir, branch, state)
    monkeypatch.chdir(target_project)

    exec_calls = []

    def mock_execvp(prog, args):
        exec_calls.append((prog, args))

    monkeypatch.setattr("os.execvp", mock_execvp)
    monkeypatch.setattr(
        "sys.argv",
        ["start-step", "--step", "6", "--branch", branch, "--", "ci", "--branch", "main"],
    )
    mod.main()

    # State was updated before exec
    updated = json.loads((state_dir / f"{branch}.json").read_text())
    assert updated["start_step"] == 6

    # execvp called with correct args
    assert len(exec_calls) == 1
    prog, args = exec_calls[0]
    assert prog.endswith("bin/flow")
    assert args[1:] == ["ci", "--branch", "main"]

    # No JSON printed to stdout (wrapping mode)
    captured = capsys.readouterr()
    assert captured.out == ""


def test_wrapping_missing_state_still_execs(target_project, state_dir, branch, monkeypatch, capsys):
    """Wrapping mode execs subcommand even when state file is missing."""
    monkeypatch.chdir(target_project)

    exec_calls = []
    monkeypatch.setattr("os.execvp", lambda prog, args: exec_calls.append((prog, args)))
    monkeypatch.setattr(
        "sys.argv",
        ["start-step", "--step", "6", "--branch", branch, "--", "ci"],
    )
    mod.main()

    # Exec still happened despite missing state file
    assert len(exec_calls) == 1
    assert exec_calls[0][1][1:] == ["ci"]


def test_wrapping_corrupt_state_still_execs(target_project, state_dir, branch, monkeypatch):
    """Wrapping mode execs subcommand even when state file is corrupt."""
    (state_dir / f"{branch}.json").write_text("broken json")
    monkeypatch.chdir(target_project)

    exec_calls = []
    monkeypatch.setattr("os.execvp", lambda prog, args: exec_calls.append((prog, args)))
    monkeypatch.setattr(
        "sys.argv",
        ["start-step", "--step", "3", "--branch", branch, "--", "log", branch, "test"],
    )
    mod.main()

    assert len(exec_calls) == 1


# --- Wrapping mode (end-to-end subprocess) ---


def test_wrapping_end_to_end(target_project, state_dir, branch):
    """End-to-end: start-step wraps set-timestamp, both updates visible in state file."""
    state = make_state()
    write_state(state_dir, branch, state)
    make_flow_json(target_project)
    bin_flow = str(BIN_DIR / "flow")
    result = subprocess.run(
        [
            bin_flow,
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
    """bin/flow start-step routes to lib/start-step.py correctly."""
    state = make_state()
    write_state(state_dir, branch, state)
    bin_flow = str(BIN_DIR / "flow")
    result = subprocess.run(
        [bin_flow, "start-step", "--step", "3", "--branch", branch],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["step"] == 3


# --- Argument validation ---


def test_missing_step_arg(target_project, monkeypatch):
    """Missing --step argument exits with error."""
    monkeypatch.chdir(target_project)
    result = _run(target_project, "--branch", "test")
    assert result.returncode == 2  # argparse exits with 2


def test_missing_branch_arg(target_project, monkeypatch):
    """Missing --branch argument exits with error."""
    monkeypatch.chdir(target_project)
    result = _run(target_project, "--step", "5")
    assert result.returncode == 2  # argparse exits with 2


# --- update_step unit tests ---


def test_update_step_returns_true(target_project, state_dir, branch, monkeypatch):
    """update_step returns True when state file exists and is updated."""
    state = make_state()
    write_state(state_dir, branch, state)
    monkeypatch.chdir(target_project)
    root = target_project
    state_path = root / ".flow-states" / f"{branch}.json"
    assert mod.update_step(state_path, 7) is True
    updated = json.loads(state_path.read_text())
    assert updated["start_step"] == 7


def test_update_step_returns_false_missing(target_project, state_dir, branch, monkeypatch):
    """update_step returns False when state file does not exist."""
    monkeypatch.chdir(target_project)
    root = target_project
    state_path = root / ".flow-states" / f"{branch}.json"
    assert mod.update_step(state_path, 7) is False
