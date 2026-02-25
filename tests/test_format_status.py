"""Tests for lib/format-status.py — the status panel formatter."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "format-status.py")

# Import format-status.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "format_status", LIB_DIR / "format-status.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

VERSION = "0.8.2"


def _run(state_path, version=VERSION):
    """Run format-status.py via subprocess."""
    result = subprocess.run(
        [sys.executable, SCRIPT, str(state_path), version],
        capture_output=True, text=True,
    )
    return result


# --- CLI behavior ---


def test_missing_args_returns_error():
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True,
    )
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


def test_nonexistent_state_returns_no_state(tmp_path):
    result = _run(tmp_path / "missing.json")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "no_state"


def test_corrupt_json_returns_error(tmp_path):
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("{bad json")
    result = _run(bad_file)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_happy_path_returns_ok_with_panel(state_dir):
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    state["phases"]["1"]["cumulative_seconds"] = 300
    path = write_state(state_dir, "test-feature", state)
    result = _run(path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert "panel" in data


# --- Panel formatting (in-process) ---


def test_panel_includes_header_with_version():
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert f"FLOW v{VERSION} — Current Status" in panel


def test_panel_includes_feature_and_branch():
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "Feature : Test Feature" in panel
    assert "Branch  : test-feature" in panel


def test_panel_includes_pr_url():
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "PR      : https://github.com/test/test/pull/1" in panel


def test_panel_shows_completed_phase_with_timing():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    state["phases"]["1"]["cumulative_seconds"] = 300
    panel = _mod.format_panel(state, VERSION)
    assert "[x] Phase 1:" in panel
    assert "(5m)" in panel


def test_panel_shows_in_progress_marker():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "[>] Phase 2:" in panel
    assert "<-- YOU ARE HERE" in panel


def test_panel_shows_pending_phases():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "[ ] Phase 3:" in panel


def test_panel_shows_current_phase_timing():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    state["phases"]["2"]["cumulative_seconds"] = 120
    state["phases"]["2"]["visit_count"] = 2
    panel = _mod.format_panel(state, VERSION)
    assert "Time in current phase : 2m" in panel
    assert "Times visited         : 2" in panel


def test_panel_shows_next_command():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Next: /flow:research" in panel


def test_panel_all_complete():
    state = make_state(
        current_phase=8,
        phase_statuses={i: "complete" for i in range(1, 9)},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "All phases complete!" in panel
    assert "This feature is fully done." in panel
    assert "Feature: Test Feature" in panel


def test_panel_timing_formats():
    state = make_state(
        current_phase=4,
        phase_statuses={1: "complete", 2: "complete", 3: "complete", 4: "in_progress"},
    )
    state["phases"]["1"]["cumulative_seconds"] = 30
    state["phases"]["2"]["cumulative_seconds"] = 3660
    state["phases"]["3"]["cumulative_seconds"] = 120
    panel = _mod.format_panel(state, VERSION)
    assert "(<1m)" in panel
    assert "(1h 1m)" in panel
    assert "(2m)" in panel


def test_panel_has_all_8_phases():
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    for i in range(1, 9):
        assert f"Phase {i}:" in panel


def test_panel_next_command_when_current_phase_not_in_progress():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "complete"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Next: /flow:design" in panel
