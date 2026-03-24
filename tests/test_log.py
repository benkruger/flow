"""Tests for lib/log.py — append log entries to .flow-states/<branch>.log."""

import fcntl
import importlib
import json
import sys
from pathlib import Path
from unittest.mock import call, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("log")


# --- append_log unit tests ---


def test_appends_to_existing_log(tmp_path):
    """Appends new entry after existing content."""
    log_dir = tmp_path / ".flow-states"
    log_dir.mkdir()
    log_file = log_dir / "my-feature.log"
    log_file.write_text("existing line\n")

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T00:00:00-08:00"):
        _mod.append_log("my-feature", "[Phase 1] Step 5 — test (exit 0)")

    content = log_file.read_text()
    assert content == (
        "existing line\n"
        "2026-01-01T00:00:00-08:00 [Phase 1] Step 5 — test (exit 0)\n"
    )


def test_creates_new_log_file(tmp_path):
    """Creates log file if it does not exist."""
    log_dir = tmp_path / ".flow-states"
    log_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-03-14T10:00:00-07:00"):
        _mod.append_log("feat-branch", "[Phase 1] test message")

    log_file = log_dir / "feat-branch.log"
    assert log_file.exists()
    assert log_file.read_text() == "2026-03-14T10:00:00-07:00 [Phase 1] test message\n"


def test_creates_directory_if_missing(tmp_path):
    """Creates .flow-states/ directory when it does not exist."""
    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T00:00:00-08:00"):
        _mod.append_log("branch", "message")

    assert (tmp_path / ".flow-states").is_dir()
    assert (tmp_path / ".flow-states" / "branch.log").exists()


def test_multiple_appends(tmp_path):
    """Multiple calls append multiple lines."""
    log_dir = tmp_path / ".flow-states"
    log_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T00:00:00-08:00"):
        _mod.append_log("branch", "first")
        _mod.append_log("branch", "second")

    content = (log_dir / "branch.log").read_text()
    lines = content.strip().split("\n")
    assert len(lines) == 2
    assert lines[0].endswith("first")
    assert lines[1].endswith("second")


def test_append_log_uses_file_locking(tmp_path):
    """append_log() must acquire fcntl.LOCK_EX before writing."""
    log_dir = tmp_path / ".flow-states"
    log_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T00:00:00-08:00"), \
         patch.object(_mod.fcntl, "flock") as mock_flock:
        _mod.append_log("branch", "test message")

    # Must have been called with LOCK_EX at least once
    lock_calls = [c for c in mock_flock.call_args_list
                  if c[0][1] == fcntl.LOCK_EX]
    assert len(lock_calls) == 1, (
        f"Expected exactly 1 LOCK_EX call, got {len(lock_calls)}: "
        f"{mock_flock.call_args_list}"
    )


# --- CLI integration ---


def test_cli_integration(git_repo, monkeypatch, capsys):
    """In-process main() appends to log file."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["log", "test-branch",
                                     "[Phase 1] Step 5 — bin/dependencies (exit 0)"])
    _mod.main()

    log_file = state_dir / "test-branch.log"
    assert log_file.exists()
    content = log_file.read_text()
    assert "[Phase 1] Step 5 — bin/dependencies (exit 0)" in content


def test_cli_missing_args(monkeypatch):
    """Missing arguments exits with error."""
    monkeypatch.setattr("sys.argv", ["log"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
