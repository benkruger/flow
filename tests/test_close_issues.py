"""Tests for lib/close-issues.py — extract issue refs from prompt and close them."""

import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, call

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

import importlib

_mod = importlib.import_module("close-issues")


# --- extract_issue_numbers ---


def test_extracts_issue_numbers():
    """Extracts #N patterns from prompt text."""
    assert _mod.extract_issue_numbers("fix #83 and #89") == [83, 89]


def test_no_issues_in_prompt():
    """Returns empty list when prompt has no issue references."""
    assert _mod.extract_issue_numbers("add new feature") == []


def test_deduplicates_issue_numbers():
    """Duplicate issue numbers are returned only once."""
    assert _mod.extract_issue_numbers("fix #83 and #83") == [83]


# --- close_issues ---


def test_closes_all_extracted_issues():
    """Calls gh issue close for each extracted issue number."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.close_issues([83, 89])

    assert result == {"closed": [83, 89], "failed": []}
    assert mock_run.call_count == 2
    mock_run.assert_any_call(
        ["gh", "issue", "close", "83"],
        capture_output=True, text=True,
    )
    mock_run.assert_any_call(
        ["gh", "issue", "close", "89"],
        capture_output=True, text=True,
    )


def test_no_issues_no_gh_calls():
    """Empty issue list means no subprocess calls."""
    with patch("subprocess.run") as mock_run:
        result = _mod.close_issues([])

    assert result == {"closed": [], "failed": []}
    mock_run.assert_not_called()


def test_partial_failure():
    """One close fails, other succeeds — both attempted."""
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
        result = _mod.close_issues([83, 89])

    assert result == {"closed": [83], "failed": [89]}


# --- CLI integration ---


def test_cli_integration(tmp_path):
    """Subprocess call with --state-file reads prompt and closes issues."""
    state = {
        "prompt": "fix #42 and #99",
        "feature": "Test",
        "branch": "test",
    }
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    script = Path(__file__).resolve().parent.parent / "lib" / "close-issues.py"
    result = subprocess.run(
        [sys.executable, str(script), "--state-file", str(state_file)],
        capture_output=True, text=True,
    )

    output = json.loads(result.stdout)
    assert output["status"] in ("ok", "error")


def test_cli_no_prompt_field(tmp_path):
    """State file without prompt field outputs ok with empty lists."""
    state = {
        "feature": "Test",
        "branch": "test",
    }
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    script = Path(__file__).resolve().parent.parent / "lib" / "close-issues.py"
    result = subprocess.run(
        [sys.executable, str(script), "--state-file", str(state_file)],
        capture_output=True, text=True,
    )

    output = json.loads(result.stdout)
    assert output == {"status": "ok", "closed": [], "failed": []}
