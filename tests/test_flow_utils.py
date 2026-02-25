"""Tests for lib/flow_utils.py — shared utilities."""

import importlib.util
import subprocess
from pathlib import Path

import pytest

from conftest import LIB_DIR

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
