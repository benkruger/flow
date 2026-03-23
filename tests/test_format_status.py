"""Tests for lib/format-status.py — the status panel formatter."""

import importlib.util
import json
import subprocess
import sys
from datetime import datetime, timezone

from conftest import LIB_DIR, PHASE_ORDER, make_state, write_state
from flow_utils import elapsed_since, read_version, read_version_from

SCRIPT = str(LIB_DIR / "format-status.py")

# Import format-status.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "format_status", LIB_DIR / "format-status.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

VERSION = "0.8.2"


def _run(cwd, branch=None):
    """Run format-status.py via subprocess with optional --branch, from cwd."""
    cmd = [sys.executable, SCRIPT]
    if branch is not None:
        cmd += ["--branch", branch]
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(cwd),
    )
    return result


# --- CLI behavior ---


def test_no_branch_returns_error(tmp_path):
    """Running outside a git repo (no branch) returns exit 2 with stderr."""
    result = _run(tmp_path)
    assert result.returncode == 2
    assert "branch" in result.stderr
    assert result.stdout == ""


def test_no_state_file_returns_exit_1(git_repo):
    """No state file returns exit 1 with no stdout."""
    result = _run(git_repo)
    assert result.returncode == 1
    assert result.stdout == ""


def test_corrupt_json_returns_exit_1(state_dir, git_repo, branch):
    """Corrupt state file for current branch is treated as no state (exit 1)."""
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")
    result = _run(git_repo)
    assert result.returncode == 1
    assert result.stdout == ""


def test_happy_path_returns_panel_text(state_dir, git_repo, branch):
    """Valid state file returns exit 0 with panel text on stdout."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 300
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    assert "FLOW v" in result.stdout
    assert "Phase 1:" in result.stdout


# --- Panel formatting (in-process) ---


def test_panel_includes_header_with_version():
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert f"FLOW v{VERSION} — Current Status" in panel


def test_panel_includes_feature_and_branch():
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "Feature : Test Feature" in panel
    assert "Branch  : test-feature" in panel


def test_panel_includes_pr_url():
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "PR      : https://github.com/test/test/pull/1" in panel


def test_panel_shows_completed_phase_with_timing():
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 300
    panel = _mod.format_panel(state, VERSION)
    assert "[x] Phase 1:" in panel
    assert "(5m)" in panel


def test_panel_shows_in_progress_marker():
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "[>] Phase 2:" in panel
    assert "<-- YOU ARE HERE" in panel


def test_panel_shows_pending_phases():
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "[ ] Phase 3:" in panel


def test_panel_shows_current_phase_timing():
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-plan"]["cumulative_seconds"] = 120
    state["phases"]["flow-plan"]["session_started_at"] = None
    state["phases"]["flow-plan"]["visit_count"] = 2
    panel = _mod.format_panel(state, VERSION)
    assert "Time in current phase : 2m" in panel
    assert "Times visited         : 2" in panel


def test_in_progress_phase_shows_live_elapsed():
    """In-progress phase with session_started_at should show live elapsed, not just cumulative."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-plan"]["cumulative_seconds"] = 0
    state["phases"]["flow-plan"]["session_started_at"] = "2026-01-01T00:00:00Z"
    now = datetime(2026, 1, 1, 0, 10, 0, tzinfo=timezone.utc)
    panel = _mod.format_panel(state, VERSION, now=now)
    assert "Time in current phase : 10m" in panel


def test_in_progress_phase_adds_live_to_cumulative():
    """Live elapsed should be added to existing cumulative_seconds for display."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-plan"]["cumulative_seconds"] = 600
    state["phases"]["flow-plan"]["session_started_at"] = "2026-01-01T00:00:00Z"
    now = datetime(2026, 1, 1, 0, 5, 0, tzinfo=timezone.utc)
    panel = _mod.format_panel(state, VERSION, now=now)
    assert "Time in current phase : 15m" in panel


def test_panel_shows_elapsed_time():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["started_at"] = "2026-01-01T00:00:00Z"
    now = datetime(2026, 1, 1, 2, 0, 0, tzinfo=timezone.utc)
    panel = _mod.format_panel(state, VERSION, now=now)
    assert "Elapsed : 2h 0m" in panel


def test_panel_shows_notes_count():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["notes"] = [
        {"text": "note 1"},
        {"text": "note 2"},
        {"text": "note 3"},
    ]
    panel = _mod.format_panel(state, VERSION)
    assert "Notes   : 3" in panel


def test_panel_hides_notes_when_zero():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["notes"] = []
    panel = _mod.format_panel(state, VERSION)
    assert "Notes" not in panel


def test_panel_hides_tasks_when_no_plan():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    assert "Tasks" not in panel


def test_panel_continue_label_when_in_progress():
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Continue: /flow:flow-plan" in panel
    assert "Next:" not in panel


def test_panel_next_label_when_phase_complete():
    """After phase 2 completes, current_phase=3, so Next shows /flow:flow-code."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Next: /flow:flow-code" in panel
    assert "Continue:" not in panel


def test_panel_next_label_when_phase_pending():
    """After phase 1 completes, current_phase=2 (pending), Next shows /flow:flow-plan."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete"},
    )
    panel = _mod.format_panel(state, VERSION)
    assert "Next: /flow:flow-plan" in panel
    assert "Continue:" not in panel


def test_panel_all_complete_shows_timing():
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={k: "complete" for k in PHASE_ORDER},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 30
    state["phases"]["flow-plan"]["cumulative_seconds"] = 900
    state["phases"]["flow-code"]["cumulative_seconds"] = 3600
    state["phases"]["flow-code-review"]["cumulative_seconds"] = 870
    state["phases"]["flow-learn"]["cumulative_seconds"] = 300
    state["phases"]["flow-complete"]["cumulative_seconds"] = 20
    panel = _mod.format_panel(state, VERSION)
    assert f"FLOW v{VERSION} — All Phases Complete!" in panel
    assert "Feature : Test Feature" in panel
    assert "PR      : https://github.com/test/test/pull/1" in panel
    assert "Elapsed : 1h 35m" in panel
    for i in range(1, 7):
        assert f"[x] Phase {i}:" in panel


def test_panel_timing_formats():
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 30
    state["phases"]["flow-plan"]["cumulative_seconds"] = 3660
    state["phases"]["flow-code"]["cumulative_seconds"] = 120
    panel = _mod.format_panel(state, VERSION)
    assert "(<1m)" in panel
    assert "(1h 1m)" in panel
    assert "(2m)" in panel


def test_panel_has_all_6_phases():
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    panel = _mod.format_panel(state, VERSION)
    for i in range(1, 7):
        assert f"Phase {i}:" in panel


def test_elapsed_since_with_no_started_at():
    assert elapsed_since(None) == 0


def test_elapsed_since_with_no_now_uses_current_time():
    """elapsed_since with a valid started_at and now=None computes live elapsed."""
    result = elapsed_since("2026-01-01T00:00:00Z")
    assert result > 0


def test_read_version_returns_fallback_when_missing(tmp_path):
    assert read_version_from(tmp_path / "nonexistent.json") == "?"


# --- Fallback behavior (wrong branch) ---


def test_wrong_branch_single_feature_returns_ok(tmp_path):
    """find_state_files() falls back to the only state file; format_panel() produces a panel."""
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
    panel = _mod.format_panel(matched_state, read_version())
    assert isinstance(panel, str) and len(panel) > 0


def test_format_panel_uses_frozen_phase_config():
    """format_panel uses phase_config when provided."""
    custom_order = ["flow-start", "flow-plan"]
    custom_names = {"flow-start": "Begin", "flow-plan": "Design"}
    custom_numbers = {"flow-start": 1, "flow-plan": 2}
    custom_commands = {"flow-start": "/t:begin", "flow-plan": "/t:design"}
    config = (custom_order, custom_names, custom_numbers, custom_commands)

    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    panel = _mod.format_panel(state, VERSION, phase_config=config)
    assert "Begin" in panel
    assert "Design" in panel
    # Should NOT contain the default phase names
    assert "Code" not in panel


def test_format_panel_all_complete_uses_frozen_phase_config():
    """format_panel with phase_config works when all phases are complete."""
    custom_order = ["flow-start", "flow-plan"]
    custom_names = {"flow-start": "Begin", "flow-plan": "Design"}
    custom_numbers = {"flow-start": 1, "flow-plan": 2}
    custom_commands = {"flow-start": "/t:begin", "flow-plan": "/t:design"}
    config = (custom_order, custom_names, custom_numbers, custom_commands)

    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
    })
    panel = _mod.format_panel(state, VERSION, phase_config=config)
    assert "All Phases Complete" in panel
    assert "Begin" in panel
    assert "Design" in panel


def test_cli_uses_frozen_phases_file(state_dir, git_repo, branch):
    """CLI loads frozen phases file when it exists."""
    import shutil
    source = LIB_DIR.parent / "flow-phases.json"
    frozen = state_dir / f"{branch}-phases.json"
    state_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(source), str(frozen))

    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    assert "Plan" in result.stdout


def test_wrong_branch_multiple_features_returns_panel(state_dir, git_repo, branch):
    """When on wrong branch with multiple state files, returns panel text."""
    for name in ["feature-a", "feature-b"]:
        state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
        state["branch"] = name
        write_state(state_dir, name, state)
    result = _run(git_repo)
    assert result.returncode == 0
    assert "Multiple Features Active" in result.stdout


# --- --branch flag (subprocess) ---


def test_cli_branch_flag_uses_specified_state_file(state_dir, git_repo):
    """--branch flag finds the state file for a different branch."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    write_state(state_dir, "other-feature", state)
    result = _run(git_repo, branch="other-feature")
    assert result.returncode == 0
    assert "FLOW v" in result.stdout
    assert "Phase 1:" in result.stdout


# --- Dev mode detection ---


def test_cli_dev_mode_from_flow_json_with_backup(state_dir, git_repo, branch):
    """CLI shows [DEV MODE] when .flow.json has plugin_root_backup."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    write_state(state_dir, branch, state)
    (git_repo / ".flow.json").write_text(json.dumps({
        "flow_version": "0.39.0",
        "plugin_root": "/local/path",
        "plugin_root_backup": "/cache/path",
    }))
    result = _run(git_repo)
    assert result.returncode == 0
    assert "[DEV MODE]" in result.stdout


def test_cli_no_dev_mode_without_backup(state_dir, git_repo, branch):
    """CLI does not show [DEV MODE] when .flow.json has no plugin_root_backup."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    write_state(state_dir, branch, state)
    (git_repo / ".flow.json").write_text(json.dumps({
        "flow_version": "0.39.0",
        "plugin_root": "/cache/path",
    }))
    result = _run(git_repo)
    assert result.returncode == 0
    assert "[DEV MODE]" not in result.stdout


def test_cli_no_dev_mode_without_flow_json(state_dir, git_repo, branch):
    """CLI does not show [DEV MODE] when .flow.json does not exist."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    write_state(state_dir, branch, state)
    result = _run(git_repo)
    assert result.returncode == 0
    assert "[DEV MODE]" not in result.stdout


def test_cli_no_dev_mode_with_malformed_flow_json(state_dir, git_repo, branch):
    """CLI does not show [DEV MODE] when .flow.json is malformed."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    write_state(state_dir, branch, state)
    (git_repo / ".flow.json").write_text("{bad json")
    result = _run(git_repo)
    assert result.returncode == 0
    assert "[DEV MODE]" not in result.stdout
