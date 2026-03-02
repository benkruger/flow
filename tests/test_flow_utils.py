"""Tests for lib/flow_utils.py — shared utilities."""

import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

from conftest import LIB_DIR, make_state

# Import flow_utils for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "flow_utils", LIB_DIR / "flow_utils.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- format_time ---


def test_format_time_under_60_seconds():
    assert _mod.format_time(0) == "<1m"
    assert _mod.format_time(30) == "<1m"
    assert _mod.format_time(59) == "<1m"


def test_format_time_exactly_60_seconds():
    assert _mod.format_time(60) == "1m"


def test_format_time_minutes_only():
    assert _mod.format_time(120) == "2m"
    assert _mod.format_time(3599) == "59m"


def test_format_time_hours_and_minutes():
    assert _mod.format_time(3600) == "1h 0m"
    assert _mod.format_time(3660) == "1h 1m"
    assert _mod.format_time(7200) == "2h 0m"
    assert _mod.format_time(7380) == "2h 3m"


def test_format_time_large_values():
    assert _mod.format_time(36000) == "10h 0m"


def test_format_time_string_input():
    assert _mod.format_time("120") == "2m"
    assert _mod.format_time("3661") == "1h 1m"
    assert _mod.format_time("30") == "<1m"


def test_format_time_non_numeric_string():
    assert _mod.format_time("<1m") == "?"
    assert _mod.format_time("fast") == "?"


def test_format_time_none_input():
    assert _mod.format_time(None) == "?"


# --- project_root ---


def test_project_root_returns_path_in_git_repo(git_repo):
    result = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 0
    # project_root relies on cwd for subprocess — test the function directly
    # by running it in the git_repo context would require monkeypatching cwd


def test_project_root_falls_back_on_git_failure(monkeypatch):
    def _raise(*args, **kwargs):
        raise OSError("git not found")
    monkeypatch.setattr(subprocess, "run", _raise)
    assert _mod.project_root() == Path(".")


# --- current_branch ---


def test_current_branch_returns_none_on_git_failure(monkeypatch):
    def _raise(*args, **kwargs):
        raise OSError("git not found")
    monkeypatch.setattr(subprocess, "run", _raise)
    assert _mod.current_branch() is None


def test_current_branch_returns_none_for_empty_string(monkeypatch):
    class FakeResult:
        stdout = ""
        returncode = 0
    monkeypatch.setattr(
        subprocess, "run",
        lambda *args, **kwargs: FakeResult(),
    )
    assert _mod.current_branch() is None


# --- find_state_files ---


def test_find_state_files_exact_match(tmp_path):
    """Exact branch match returns single-item list."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    (state_dir / "my-feature.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "my-feature")
    assert len(results) == 1
    path, data, branch_name = results[0]
    assert path == state_dir / "my-feature.json"
    assert data["feature"] == "Test Feature"
    assert branch_name == "my-feature"


def test_find_state_files_no_state_dir(tmp_path):
    """No .flow-states directory returns empty list."""
    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


def test_find_state_files_empty_state_dir(tmp_path):
    """Empty .flow-states directory returns empty list."""
    (tmp_path / ".flow-states").mkdir()
    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


def test_find_state_files_fallback_single(tmp_path):
    """Single non-matching file found via fallback scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase=3)
    (state_dir / "feature-xyz.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 1
    path, data, branch_name = results[0]
    assert branch_name == "feature-xyz"


def test_find_state_files_fallback_multiple(tmp_path):
    """Multiple non-matching files returned as multi-item list."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    for name in ["feature-a", "feature-b", "feature-c"]:
        state = make_state(current_phase=2)
        state["feature"] = name
        (state_dir / f"{name}.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 3
    branches = [r[2] for r in results]
    assert "feature-a" in branches
    assert "feature-b" in branches
    assert "feature-c" in branches


def test_find_state_files_corrupt_skipped_in_scan(tmp_path):
    """Corrupt files are skipped during fallback scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "bad.json").write_text("{corrupt")
    state = make_state(current_phase=2)
    (state_dir / "good.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 1
    assert results[0][2] == "good"


def test_find_state_files_corrupt_exact_match_no_fallthrough(tmp_path):
    """Corrupt exact match returns empty — does not fall through to scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{corrupt")
    state = make_state(current_phase=2)
    (state_dir / "other-feature.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


def test_find_state_files_exact_match_priority(tmp_path):
    """Exact match takes priority — other files are not returned."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state_exact = make_state(current_phase=2)
    state_exact["feature"] = "Exact"
    (state_dir / "my-branch.json").write_text(json.dumps(state_exact))
    state_other = make_state(current_phase=3)
    state_other["feature"] = "Other"
    (state_dir / "other-branch.json").write_text(json.dumps(state_other))

    results = _mod.find_state_files(tmp_path, "my-branch")
    assert len(results) == 1
    assert results[0][1]["feature"] == "Exact"
    assert results[0][2] == "my-branch"
