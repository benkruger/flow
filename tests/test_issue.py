"""Tests for lib/issue.py — GitHub issue creation wrapper."""

import json
import subprocess
from unittest.mock import patch

import pytest

from conftest import LIB_DIR

# Import the module under test
import importlib.util

spec = importlib.util.spec_from_file_location("issue", LIB_DIR / "issue.py")
issue_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(issue_mod)


class TestDetectRepo:
    """Tests for the detect_repo function."""

    def test_ssh_url_with_dotgit(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="git@github.com:owner/repo.git\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() == "owner/repo"

    def test_https_url_with_dotgit(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo.git\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() == "owner/repo"

    def test_https_url_without_dotgit(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() == "owner/repo"

    def test_ssh_url_without_dotgit(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="git@github.com:owner/repo\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() == "owner/repo"

    def test_non_github_url_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://gitlab.com/owner/repo.git\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() is None

    def test_git_failure_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="fatal: not a git repository",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() is None

    def test_empty_output_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() is None

    def test_malformed_url_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="not-a-url\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            assert issue_mod.detect_repo() is None

    def test_subprocess_exception_returns_none(self):
        with patch.object(
            issue_mod.subprocess, "run", side_effect=OSError("git not found"),
        ):
            assert issue_mod.detect_repo() is None


class TestCreateIssue:
    """Tests for the create_issue function."""

    def test_happy_path_with_all_args(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/42\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result) as mock_run:
            url, error = issue_mod.create_issue(
                "owner/repo", "Test title", label="bug", body="Test body",
            )

        assert url == "https://github.com/owner/repo/issues/42"
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "issue", "create", "--repo", "owner/repo",
             "--title", "Test title", "--label", "bug", "--body", "Test body"],
            capture_output=True, text=True,
        )

    def test_happy_path_minimal_args(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/1\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result) as mock_run:
            url, error = issue_mod.create_issue("owner/repo", "Title only")

        assert url == "https://github.com/owner/repo/issues/1"
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "issue", "create", "--repo", "owner/repo",
             "--title", "Title only"],
            capture_output=True, text=True,
        )

    def test_label_only_no_body(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/5\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result) as mock_run:
            url, error = issue_mod.create_issue(
                "owner/repo", "With label", label="enhancement",
            )

        assert url == "https://github.com/owner/repo/issues/5"
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "issue", "create", "--repo", "owner/repo",
             "--title", "With label", "--label", "enhancement"],
            capture_output=True, text=True,
        )

    def test_body_only_no_label(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/7\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result) as mock_run:
            url, error = issue_mod.create_issue(
                "owner/repo", "With body", body="Details here",
            )

        assert url == "https://github.com/owner/repo/issues/7"
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "issue", "create", "--repo", "owner/repo",
             "--title", "With body", "--body", "Details here"],
            capture_output=True, text=True,
        )

    def test_gh_failure_stderr(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="HTTP 422: Validation Failed",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            url, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert url is None
        assert error == "HTTP 422: Validation Failed"

    def test_gh_failure_stdout_fallback(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="Something went wrong",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            url, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert url is None
        assert error == "Something went wrong"

    def test_gh_failure_unknown(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            url, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert url is None
        assert error == "Unknown error"


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_success(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/10\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test", "--label", "bug",
                                "--body", "Body text"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/owner/repo/issues/10"

    def test_main_failure(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="Auth required",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test"]), \
             pytest.raises(SystemExit, match="1"):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"
        assert output["message"] == "Auth required"

    def test_main_auto_detect_repo(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/detected/repo/issues/99\n",
            stderr="",
        )
        with patch.object(issue_mod, "detect_repo", return_value="detected/repo"), \
             patch.object(issue_mod.subprocess, "run", return_value=fake_result), \
             patch("sys.argv", ["issue.py", "--title", "Auto detected"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/detected/repo/issues/99"

    def test_main_explicit_repo_overrides(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/explicit/repo/issues/1\n",
            stderr="",
        )
        with patch.object(issue_mod, "detect_repo") as mock_detect, \
             patch.object(issue_mod.subprocess, "run", return_value=fake_result), \
             patch("sys.argv", ["issue.py", "--repo", "explicit/repo",
                                "--title", "Explicit"]):
            issue_mod.main()

        mock_detect.assert_not_called()
        output = json.loads(capsys.readouterr().out)
        assert output["url"] == "https://github.com/explicit/repo/issues/1"

    def test_main_auto_detect_fails(self, capsys):
        with patch.object(issue_mod, "detect_repo", return_value=None), \
             patch("sys.argv", ["issue.py", "--title", "No repo"]), \
             pytest.raises(SystemExit, match="1"):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"
        assert "--repo" in output["message"]

    def test_main_missing_title(self):
        with patch("sys.argv", ["issue.py", "--repo", "owner/repo"]), \
             pytest.raises(SystemExit, match="2"):
            issue_mod.main()
