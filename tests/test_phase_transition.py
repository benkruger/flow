"""Tests for lib/phase-transition.py — phase entry and completion."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "phase-transition.py")


def _run(git_repo, phase, action, next_phase=None):
    """Run phase-transition.py with the given args."""
    cmd = [sys.executable, SCRIPT, "--phase", str(phase), "--action", action]
    if next_phase is not None:
        cmd += ["--next-phase", str(next_phase)]
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


def _read_state(state_dir, branch):
    """Read and parse the state file."""
    return json.loads((state_dir / f"{branch}.json").read_text())


# --- Phase entry ---


def test_enter_sets_all_fields(git_repo, state_dir, branch):
    """Enter sets status, started_at, session_started_at, visit_count, current_phase."""
    state = make_state(current_phase=1, phase_statuses={1: "complete"})
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "enter")
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == 2
    assert output["action"] == "enter"
    assert output["visit_count"] == 1
    assert output["first_visit"] is True

    updated = _read_state(state_dir, branch)
    assert updated["phases"]["2"]["status"] == "in_progress"
    assert updated["phases"]["2"]["started_at"] is not None
    assert updated["phases"]["2"]["session_started_at"] is not None
    assert updated["phases"]["2"]["visit_count"] == 1
    assert updated["current_phase"] == 2


def test_enter_first_visit_sets_started_at(git_repo, state_dir, branch):
    """First visit sets started_at when it is null."""
    state = make_state(current_phase=1, phase_statuses={1: "complete"})
    assert state["phases"]["2"]["started_at"] is None
    write_state(state_dir, branch, state)

    _run(git_repo, 2, "enter")

    updated = _read_state(state_dir, branch)
    assert updated["phases"]["2"]["started_at"] is not None


def test_enter_reentry_preserves_started_at(git_repo, state_dir, branch):
    """Re-entry preserves started_at and increments visit_count."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "complete"})
    state["phases"]["2"]["started_at"] = "2026-01-15T10:00:00Z"
    state["phases"]["2"]["visit_count"] = 2
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "enter")
    output = json.loads(result.stdout)
    assert output["visit_count"] == 3
    assert output["first_visit"] is False

    updated = _read_state(state_dir, branch)
    assert updated["phases"]["2"]["started_at"] == "2026-01-15T10:00:00Z"
    assert updated["phases"]["2"]["visit_count"] == 3


# --- Phase completion ---


def test_complete_sets_all_fields(git_repo, state_dir, branch):
    """Complete sets cumulative_seconds, status, completed_at, session_started_at=null, current_phase."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == 2
    assert output["action"] == "complete"
    assert "cumulative_seconds" in output
    assert "formatted_time" in output
    assert output["next_phase"] == 3

    updated = _read_state(state_dir, branch)
    assert updated["phases"]["2"]["status"] == "complete"
    assert updated["phases"]["2"]["completed_at"] is not None
    assert updated["phases"]["2"]["session_started_at"] is None
    assert updated["current_phase"] == 3


def test_complete_adds_to_existing_cumulative(git_repo, state_dir, branch):
    """Complete adds elapsed time to existing cumulative_seconds."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["cumulative_seconds"] = 600
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    output = json.loads(result.stdout)
    assert output["cumulative_seconds"] >= 600


def test_complete_formatted_time_less_than_one_minute(git_repo, state_dir, branch):
    """Formatted time shows <1m for short durations."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["cumulative_seconds"] = 0
    state["phases"]["2"]["session_started_at"] = None
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    output = json.loads(result.stdout)
    assert output["formatted_time"] == "<1m"


def test_complete_next_phase_override(git_repo, state_dir, branch):
    """--next-phase overrides the default phase+1."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete", next_phase=4)
    output = json.loads(result.stdout)
    assert output["next_phase"] == 4

    updated = _read_state(state_dir, branch)
    assert updated["current_phase"] == 4


def test_complete_null_session_started_at(git_repo, state_dir, branch):
    """Null session_started_at on complete results in elapsed=0."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["session_started_at"] = None
    state["phases"]["2"]["cumulative_seconds"] = 100
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    output = json.loads(result.stdout)
    assert output["cumulative_seconds"] == 100


# --- Formatted time values ---


def test_formatted_time_minutes(git_repo, state_dir, branch):
    """Formatted time shows Xm for >= 60 seconds."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["cumulative_seconds"] = 300
    state["phases"]["2"]["session_started_at"] = None
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    output = json.loads(result.stdout)
    assert output["formatted_time"] == "5m"


def test_formatted_time_hours(git_repo, state_dir, branch):
    """Formatted time shows Xh Ym for >= 3600 seconds."""
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["cumulative_seconds"] = 3900
    state["phases"]["2"]["session_started_at"] = None
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "complete")
    output = json.loads(result.stdout)
    assert output["formatted_time"] == "1h 5m"


# --- Error cases ---


def test_error_missing_state_file(git_repo):
    """Missing state file returns error."""
    result = _run(git_repo, 2, "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "No state file" in output["message"]


def test_error_invalid_phase(git_repo, state_dir, branch):
    """Invalid phase number returns error."""
    state = make_state(current_phase=1)
    write_state(state_dir, branch, state)

    result = _run(git_repo, 8, "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Invalid phase" in output["message"]


def test_error_phase_not_in_state(git_repo, state_dir, branch):
    """Phase key missing from state phases dict returns error."""
    state = {"feature": "Test", "branch": branch, "current_phase": 1, "phases": {}}
    write_state(state_dir, branch, state)

    result = _run(git_repo, 2, "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "not found" in output["message"]


def test_error_corrupt_json(git_repo, state_dir, branch):
    """Corrupt JSON state file returns error."""
    state_dir.mkdir(parents=True, exist_ok=True)
    (state_dir / f"{branch}.json").write_text("{bad json")

    result = _run(git_repo, 2, "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Could not read" in output["message"]


def test_error_detached_head(git_repo, state_dir, branch):
    """Detached HEAD returns error."""
    state = make_state(current_phase=1)
    write_state(state_dir, branch, state)

    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    result = _run(git_repo, 2, "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "branch" in output["message"]


# --- Unit test for edge case ---


def test_complete_future_session_started_clamps_to_zero():
    """If session_started_at is in the future, elapsed clamps to 0."""
    spec = importlib.util.spec_from_file_location(
        "phase_transition", LIB_DIR / "phase-transition.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["phases"]["2"]["session_started_at"] = "2099-12-31T23:59:59Z"
    state["phases"]["2"]["cumulative_seconds"] = 50

    updated, result = mod.phase_complete(state, 2)
    assert result["cumulative_seconds"] == 50
