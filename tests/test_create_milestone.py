"""Tests for lib/create-milestone.py — GitHub milestone creation wrapper."""

import importlib.util
import json
import subprocess
from unittest.mock import patch

import pytest
from conftest import LIB_DIR

spec = importlib.util.spec_from_file_location("create_milestone", LIB_DIR / "create-milestone.py")
milestone_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(milestone_mod)


class TestCreateMilestone:
    """Tests for the create_milestone function."""

    def test_happy_path(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"number": 5, "html_url": "https://github.com/owner/repo/milestone/5"}\n',
            stderr="",
        )
        with patch.object(milestone_mod.subprocess, "run", return_value=fake_result) as mock_run:
            result, error = milestone_mod.create_milestone(
                "owner/repo",
                "v1.0 Release",
                "2026-06-01",
            )

        assert error is None
        assert result["number"] == 5
        assert "milestone/5" in result["url"]
        mock_run.assert_called_once_with(
            [
                "gh",
                "api",
                "repos/owner/repo/milestones",
                "--method",
                "POST",
                "-f",
                "title=v1.0 Release",
                "-f",
                "due_on=2026-06-01T00:00:00Z",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_gh_api_failure(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Validation Failed",
        )
        with patch.object(milestone_mod.subprocess, "run", return_value=fake_result):
            result, error = milestone_mod.create_milestone(
                "owner/repo",
                "Bad",
                "2026-01-01",
            )

        assert result is None
        assert "Validation Failed" in error

    def test_timeout(self):
        with patch.object(milestone_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            result, error = milestone_mod.create_milestone(
                "owner/repo",
                "Slow",
                "2026-01-01",
            )

        assert result is None
        assert "timed out" in error.lower()

    def test_missing_number_field(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"html_url": "https://github.com/o/r/milestone/1"}\n',
            stderr="",
        )
        with patch.object(milestone_mod.subprocess, "run", return_value=fake_result):
            result, error = milestone_mod.create_milestone(
                "owner/repo",
                "Test",
                "2026-01-01",
            )

        assert result is None
        assert "missing" in error.lower()

    def test_invalid_json_response(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not json\n",
            stderr="",
        )
        with patch.object(milestone_mod.subprocess, "run", return_value=fake_result):
            result, error = milestone_mod.create_milestone(
                "owner/repo",
                "Test",
                "2026-01-01",
            )

        assert result is None
        assert "Invalid" in error


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_success(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"number": 3, "html_url": "https://github.com/o/r/milestone/3"}\n',
            stderr="",
        )
        with (
            patch.object(milestone_mod.subprocess, "run", return_value=fake_result),
            patch(
                "sys.argv", ["create-milestone.py", "--repo", "o/r", "--title", "Sprint 1", "--due-date", "2026-04-01"]
            ),
        ):
            milestone_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["number"] == 3

    def test_main_failure(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with (
            patch.object(milestone_mod.subprocess, "run", return_value=fake_result),
            patch("sys.argv", ["create-milestone.py", "--repo", "o/r", "--title", "Bad", "--due-date", "2026-01-01"]),
            pytest.raises(SystemExit, match="1"),
        ):
            milestone_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"

    def test_main_missing_required_args(self):
        with patch("sys.argv", ["create-milestone.py", "--repo", "o/r"]), pytest.raises(SystemExit, match="2"):
            milestone_mod.main()
