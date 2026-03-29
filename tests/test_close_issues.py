"""Tests for lib/close-issues.py — extract issue refs from prompt and close them."""

import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

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


def test_extracts_issue_numbers_from_urls():
    """Extracts issue numbers from GitHub URL format."""
    assert _mod.extract_issue_numbers("fix https://github.com/owner/repo/issues/42") == [42]


def test_extracts_mixed_hash_and_url():
    """Extracts issue numbers from mixed #N and URL formats."""
    result = _mod.extract_issue_numbers("fix #83 and https://github.com/owner/repo/issues/89")
    assert result == [83, 89]


# --- close_issues ---


def test_closes_all_extracted_issues_with_repo():
    """Calls gh issue close for each issue, includes URLs when repo provided."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="",
            stderr="",
        )
        result = _mod.close_issues([83, 89], repo="test/test")

    assert result == {
        "closed": [
            {"number": 83, "url": "https://github.com/test/test/issues/83"},
            {"number": 89, "url": "https://github.com/test/test/issues/89"},
        ],
        "failed": [],
    }
    assert mock_run.call_count == 2
    mock_run.assert_any_call(
        ["gh", "issue", "close", "83", "--repo", "test/test"],
        capture_output=True,
        text=True,
        timeout=30,
    )
    mock_run.assert_any_call(
        ["gh", "issue", "close", "89", "--repo", "test/test"],
        capture_output=True,
        text=True,
        timeout=30,
    )


def test_closes_issues_without_repo():
    """When repo is None, closed items have number but no url, and no --repo flag."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="",
            stderr="",
        )
        result = _mod.close_issues([42])

    assert result == {
        "closed": [{"number": 42}],
        "failed": [],
    }
    # Verify --repo is NOT in the command args
    call_args = mock_run.call_args[0][0]
    assert "--repo" not in call_args


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
                args=args,
                returncode=0,
                stdout="",
                stderr="",
            )
        return subprocess.CompletedProcess(
            args=args,
            returncode=1,
            stdout="",
            stderr="not found",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.close_issues([83, 89], repo="test/test")

    assert result == {
        "closed": [
            {"number": 83, "url": "https://github.com/test/test/issues/83"},
        ],
        "failed": [{"number": 89, "error": "not found"}],
    }


def test_timeout_counts_as_failure():
    """TimeoutExpired on gh call adds issue to failed list."""
    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
        result = _mod.close_issues([42], repo="test/test")

    assert result == {"closed": [], "failed": [{"number": 42, "error": "timeout"}]}


# --- CLI integration ---


def test_cli_integration(tmp_path, monkeypatch, capsys):
    """In-process main() with --state-file reads prompt and closes issues."""
    state = {
        "prompt": "fix #42 and #99",
        "repo": "test/test",
        "branch": "test",
    }
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["close-issues.py", "--state-file", str(state_file)])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] in ("ok", "error")


def test_cli_no_prompt_field(tmp_path, monkeypatch, capsys):
    """State file without prompt field outputs ok with empty lists."""
    state = {
        "branch": "test",
    }
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["close-issues.py", "--state-file", str(state_file)])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output == {"status": "ok", "closed": [], "failed": []}


def test_cli_corrupt_state_file(tmp_path, monkeypatch, capsys):
    """Corrupt state file returns structured error."""
    state_file = tmp_path / "state.json"
    state_file.write_text("{corrupt")

    monkeypatch.setattr("sys.argv", ["close-issues.py", "--state-file", str(state_file)])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"
    assert "state file" in output["message"].lower()
