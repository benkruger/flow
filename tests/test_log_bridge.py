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


def test_cli_delegates_to_append_log(monkeypatch):
    """main() delegates to append_log with correct args."""
    monkeypatch.setattr("sys.argv", ["log", "test-branch", "test message"])
    with patch.object(_mod, "append_log") as mock:
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
