"""Tests for lib/log.py — bridge to Rust log command."""

import importlib
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("log")


def test_cli_missing_args(monkeypatch):
    """Missing arguments exits with error."""
    monkeypatch.setattr("sys.argv", ["log"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1


def test_cli_delegates_to_direct_append(monkeypatch):
    """main() delegates to _direct_append (not append_log) to avoid recursion."""
    monkeypatch.setattr("sys.argv", ["log", "test-branch", "test message"])
    with patch.object(_mod, "_direct_append") as mock:
        _mod.main()
    mock.assert_called_once_with("test-branch", "test message")


def test_append_log_calls_subprocess(monkeypatch):
    """append_log calls bin/flow log via subprocess."""
    with patch.object(_mod.subprocess, "run") as mock_run:
        _mod.append_log("my-branch", "hello")
    mock_run.assert_called_once()
    args = mock_run.call_args[0][0]
    assert args[0].endswith("bin/flow")
    assert args[1:] == ["log", "my-branch", "hello"]


def test_direct_append_writes_log_file(git_repo, monkeypatch):
    """_direct_append writes timestamped line to .flow-states/<branch>.log."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    monkeypatch.chdir(git_repo)
    _mod._direct_append("test-branch", "[Phase 1] test message")
    log_file = state_dir / "test-branch.log"
    assert log_file.exists()
    content = log_file.read_text()
    assert "[Phase 1] test message" in content
