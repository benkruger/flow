"""Tests for lib/format-status.py — the status panel formatter."""

import importlib.util
import json
import subprocess
import sys
from datetime import datetime, timezone

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "format-status.py")

# Import format-status.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "format_status", LIB_DIR / "format-status.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

VERSION = "0.8.2"


def _run(cwd):
    """Run format-status.py via subprocess with no args, from cwd."""
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


def test_corrupt_json_returns_error(state_dir, git_repo, branch):
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")
    result = _run(git_repo)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_happy_path_returns_ok_with_panel(state_dir, git_repo, branch):
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


def test_panel_shows_elapsed_time():
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["started_at"] = "2026-01-01T00:00:00Z"
    now = datetime(2026, 1, 1, 2, 0, 0, tzinfo=timezone.utc)
    panel = _mod.format_panel(state, VERSION, now=now)
    assert "Elapsed : 2h 0m" in panel


def test_panel_shows_notes_count():
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["notes"] = [
        {"text": "note 1"},
        {"text": "note 2"},
        {"text": "note 3"},
    ]
    panel = _mod.format_panel(state, VERSION)
    assert "Notes   : 3" in panel


def test_panel_hides_notes_when_zero():
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    state["notes"] = []
    panel = _mod.format_panel(state, VERSION)
    assert "Notes" not in panel


def test_panel_shows_plan_progress():
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete", 5: "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"status": "complete"},
            {"status": "complete"},
            {"status": "complete"},
            {"status": "pending"},
            {"status": "pending"},
            {"status": "pending"},
            {"status": "pending"},
        ],
    }
    panel = _mod.format_panel(state, VERSION)
    assert "Tasks   : 3/7 complete" in panel


def test_panel_hides_plan_when_absent():
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "Tasks" not in panel


def test_panel_continue_label_when_in_progress():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Continue: /flow:research" in panel
    assert "Next:" not in panel


def test_panel_next_label_when_phase_complete():
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "complete"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Next: /flow:design" in panel
    assert "Continue:" not in panel


def test_panel_all_complete_shows_timing():
    state = make_state(
        current_phase=9,
        phase_statuses={i: "complete" for i in range(1, 10)},
    )
    state["phases"]["1"]["cumulative_seconds"] = 30
    state["phases"]["2"]["cumulative_seconds"] = 900
    state["phases"]["3"]["cumulative_seconds"] = 600
    state["phases"]["4"]["cumulative_seconds"] = 1200
    state["phases"]["5"]["cumulative_seconds"] = 3600
    state["phases"]["6"]["cumulative_seconds"] = 450
    state["phases"]["7"]["cumulative_seconds"] = 300
    state["phases"]["8"]["cumulative_seconds"] = 300
    state["phases"]["9"]["cumulative_seconds"] = 20
    panel = _mod.format_panel(state, VERSION)
    assert f"FLOW v{VERSION} — All Phases Complete!" in panel
    assert "Feature : Test Feature" in panel
    assert "PR      : https://github.com/test/test/pull/1" in panel
    assert "Elapsed : 2h 3m" in panel
    for i in range(1, 10):
        assert f"[x] Phase {i}:" in panel


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


def test_panel_has_all_9_phases():
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    for i in range(1, 10):
        assert f"Phase {i}:" in panel


def test_panel_shows_skipped_phase():
    """Skipped phase (light mode) shows [~] marker and (skipped) label."""
    state = make_state(
        current_phase=4,
        phase_statuses={1: "complete", 2: "complete", 3: "complete", 4: "in_progress"},
        mode="light",
    )
    state["phases"]["3"]["skipped"] = True
    state["phases"]["3"]["cumulative_seconds"] = 0
    state["phases"]["3"]["visit_count"] = 0
    panel = _mod.format_panel(state, VERSION)
    assert "[~] Phase 3:" in panel
    assert "(skipped)" in panel


def test_panel_skipped_phase_in_all_complete():
    """Skipped phase in all-complete panel shows [~] and (skipped)."""
    state = make_state(
        current_phase=9,
        phase_statuses={i: "complete" for i in range(1, 10)},
        mode="light",
    )
    state["phases"]["3"]["skipped"] = True
    state["phases"]["3"]["cumulative_seconds"] = 0
    for i in range(1, 10):
        if i != 3:
            state["phases"][str(i)]["cumulative_seconds"] = 60
    panel = _mod.format_panel(state, VERSION)
    assert "[~] Phase 3:" in panel
    assert "(skipped)" in panel


def test_elapsed_since_with_no_started_at():
    assert _mod._elapsed_since(None) == 0


def test_read_version_returns_fallback_when_missing(tmp_path, monkeypatch):
    monkeypatch.setattr(_mod, "__file__", str(tmp_path / "lib" / "format-status.py"))
    assert _mod._read_version() == "?"
