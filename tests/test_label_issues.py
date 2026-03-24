"""Tests for lib/label-issues.py — add/remove Flow In-Progress label on issues."""

import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

import importlib

_mod = importlib.import_module("label-issues")

from conftest import make_state


# --- label_issues (add) ---


def test_add_label_to_single_issue():
    """Adds Flow In-Progress label to a single issue referenced in prompt."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.label_issues([83], action="add")

    assert result == {"labeled": [83], "failed": []}
    mock_run.assert_called_once_with(
        ["gh", "issue", "edit", "83", "--add-label", "Flow In-Progress"],
        capture_output=True, text=True, timeout=30,
    )


def test_remove_label_from_single_issue():
    """Removes Flow In-Progress label from a single issue."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.label_issues([83], action="remove")

    assert result == {"labeled": [83], "failed": []}
    mock_run.assert_called_once_with(
        ["gh", "issue", "edit", "83", "--remove-label", "Flow In-Progress"],
        capture_output=True, text=True, timeout=30,
    )


def test_multiple_issues_in_prompt():
    """Labels multiple issues referenced in the prompt."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.label_issues([83, 89], action="add")

    assert result == {"labeled": [83, 89], "failed": []}
    assert mock_run.call_count == 2


def test_no_issues_no_gh_calls():
    """Empty issue list means no subprocess calls."""
    with patch("subprocess.run") as mock_run:
        result = _mod.label_issues([], action="add")

    assert result == {"labeled": [], "failed": []}
    mock_run.assert_not_called()


def test_partial_failure():
    """One label succeeds, one fails — both attempted."""
    def side_effect(args, **kwargs):
        issue_num = args[3]
        if issue_num == "83":
            return subprocess.CompletedProcess(
                args=args, returncode=0, stdout="", stderr="",
            )
        return subprocess.CompletedProcess(
            args=args, returncode=1, stdout="", stderr="not found",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.label_issues([83, 89], action="add")

    assert result == {"labeled": [83], "failed": [89]}


def test_timeout_counts_as_failure():
    """TimeoutExpired on gh call adds issue to failed list."""
    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
        result = _mod.label_issues([42], action="add")

    assert result == {"labeled": [], "failed": [42]}


# --- main (CLI integration) ---


def test_cli_integration_add(tmp_path, monkeypatch, capsys):
    """In-process main() with --add returns valid JSON with status ok."""
    state = make_state()
    state["prompt"] = "fix #42"
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["label-issues", "--state-file", str(state_file), "--add"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "ok"
    assert "labeled" in output
    assert "failed" in output


def test_cli_integration_remove(tmp_path, monkeypatch, capsys):
    """In-process main() with --remove returns valid JSON with status ok."""
    state = make_state()
    state["prompt"] = "fix #42"
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["label-issues", "--state-file", str(state_file), "--remove"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "ok"
    assert "labeled" in output
    assert "failed" in output


def test_missing_prompt_field(tmp_path, monkeypatch, capsys):
    """State file without prompt field outputs ok with empty lists."""
    state = make_state()
    del state["prompt"]
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["label-issues", "--state-file", str(state_file), "--add"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output == {"status": "ok", "labeled": [], "failed": []}
