"""Tests for lib/close-issue.py — close a single GitHub issue."""

import io
import json
import subprocess
import sys
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

import importlib

_mod = importlib.import_module("close-issue")


# --- close_issue_by_number ---


def test_closes_single_issue():
    """Calls gh issue close for the specified issue."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.close_issue_by_number("benkruger/flow", 117)

    assert result is None
    mock_run.assert_called_once_with(
        ["gh", "issue", "close", "--repo", "benkruger/flow", "117"],
        capture_output=True, text=True,
    )


def test_close_issue_failure():
    """Returns error message when gh issue close fails."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Issue not found",
        )
        result = _mod.close_issue_by_number("benkruger/flow", 999)

    assert result == "Issue not found"


def test_close_issue_no_stderr():
    """Uses stdout as error message when stderr is empty."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="Not found", stderr="",
        )
        result = _mod.close_issue_by_number("benkruger/flow", 999)

    assert result == "Not found"


def test_close_issue_generic_error():
    """Returns generic error when both stdout and stderr are empty."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="",
        )
        result = _mod.close_issue_by_number("benkruger/flow", 999)

    assert result == "Unknown error"


# --- detect_repo_or_fail ---


def test_detects_repo_from_git_remote():
    """Auto-detects repo from git remote origin URL."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/benkruger/flow.git\n",
            stderr="",
        )
        result = _mod.detect_repo_or_fail()

    assert result == "benkruger/flow"


def test_detects_repo_ssh_format():
    """Auto-detects repo from SSH git remote format."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="git@github.com:benkruger/flow.git",
            stderr="",
        )
        result = _mod.detect_repo_or_fail()

    assert result == "benkruger/flow"


def test_detects_repo_without_git_suffix():
    """Auto-detects repo when URL has no .git suffix."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/benkruger/flow",
            stderr="",
        )
        result = _mod.detect_repo_or_fail()

    assert result == "benkruger/flow"


def test_detection_fails_when_no_remote():
    """Raises SystemExit when git remote returns error."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="No such remote",
        )
        with pytest.raises(SystemExit) as exc_info:
            _mod.detect_repo_or_fail()
        assert exc_info.value.code == 1


def test_detection_fails_when_not_github():
    """Raises SystemExit when remote is not GitHub."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://gitlab.com/user/repo.git",
            stderr="",
        )
        with pytest.raises(SystemExit) as exc_info:
            _mod.detect_repo_or_fail()
        assert exc_info.value.code == 1


def test_detection_fails_when_url_is_empty():
    """Raises SystemExit when git remote returns empty string."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        with pytest.raises(SystemExit) as exc_info:
            _mod.detect_repo_or_fail()
        assert exc_info.value.code == 1


def test_detection_fails_on_exception():
    """Raises SystemExit when git command raises exception."""
    with patch("subprocess.run") as mock_run:
        mock_run.side_effect = Exception("subprocess error")
        with pytest.raises(SystemExit) as exc_info:
            _mod.detect_repo_or_fail()
        assert exc_info.value.code == 1


# --- CLI integration (via direct main() calls with mocked subprocess) ---


def test_cli_closes_issue_with_number_and_repo():
    """CLI with --number and --repo closes the issue via gh."""
    def mock_run(args, **kwargs):
        if args[0] == "gh" and args[1] == "issue" and args[2] == "close":
            return subprocess.CompletedProcess(
                args=args, returncode=0, stdout="", stderr="",
            )
        return subprocess.CompletedProcess(
            args=args, returncode=1, stdout="", stderr="Unknown command",
        )

    with patch.object(_mod, "subprocess") as mock_subprocess:
        with patch.object(sys, "argv", ["close-issue.py", "--number", "117", "--repo", "benkruger/flow"]):
            mock_subprocess.run = mock_run

            output_text = io.StringIO()
            with redirect_stdout(output_text):
                try:
                    _mod.main()
                except SystemExit as e:
                    if e.code != 0:
                        raise

            result = json.loads(output_text.getvalue())
            assert result["status"] == "ok"


def test_cli_requires_number_argument():
    """CLI fails when --number is not provided."""
    with patch.object(sys, "argv", ["close-issue.py", "--repo", "benkruger/flow"]):
        with pytest.raises(SystemExit) as exc_info:
            _mod.main()
        assert exc_info.value.code == 2


def test_cli_auto_detects_repo():
    """CLI closes issue with auto-detected repo when --repo is omitted."""
    def mock_run(args, **kwargs):
        if args[0] == "git" and args[1] == "remote":
            return subprocess.CompletedProcess(
                args=args, returncode=0,
                stdout="https://github.com/benkruger/flow.git\n",
                stderr="",
            )
        # gh issue close
        return subprocess.CompletedProcess(
            args=args, returncode=0, stdout="", stderr="",
        )

    with patch.object(_mod, "subprocess") as mock_subprocess:
        with patch.object(sys, "argv", ["close-issue.py", "--number", "117"]):
            mock_subprocess.run = mock_run

            output_text = io.StringIO()
            with redirect_stdout(output_text):
                try:
                    _mod.main()
                except SystemExit as e:
                    if e.code != 0:
                        raise

            result = json.loads(output_text.getvalue())
            assert result["status"] == "ok"


def test_cli_error_when_detection_fails():
    """CLI outputs error when repo detection fails."""
    with patch.object(_mod, "detect_repo", return_value=None):
        with patch.object(sys, "argv", ["close-issue.py", "--number", "117"]):
            output_text = io.StringIO()
            with redirect_stdout(output_text):
                with pytest.raises(SystemExit):
                    _mod.main()

            result = json.loads(output_text.getvalue())
            assert result["status"] == "error"
            assert "Could not detect repo" in result["message"]


def test_cli_error_from_gh_issue_close():
    """CLI outputs error when gh issue close fails."""
    def mock_run(args, **kwargs):
        return subprocess.CompletedProcess(
            args=args, returncode=1, stdout="", stderr="Issue 999 not found",
        )

    with patch.object(_mod, "subprocess") as mock_subprocess:
        with patch.object(sys, "argv", ["close-issue.py", "--number", "999", "--repo", "benkruger/flow"]):
            mock_subprocess.run = mock_run

            output_text = io.StringIO()
            with redirect_stdout(output_text):
                with pytest.raises(SystemExit):
                    _mod.main()

            result = json.loads(output_text.getvalue())
            assert result["status"] == "error"
            assert result["message"] == "Issue 999 not found"
