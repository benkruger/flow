"""Tests for lib/issue.py — GitHub issue creation wrapper."""

import json
import re
import subprocess
from unittest.mock import patch, call

import pytest

from conftest import LIB_DIR

# Import the module under test
import importlib.util

spec = importlib.util.spec_from_file_location("issue", LIB_DIR / "issue.py")
issue_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(issue_mod)



class TestReadBodyFile:
    """Tests for the read_body_file function."""

    def test_reads_and_deletes_file(self, tmp_path):
        body_file = tmp_path / ".flow-issue-body"
        body_file.write_text("Issue body with | pipes and && ampersands")

        body, error = issue_mod.read_body_file(str(body_file))

        assert body == "Issue body with | pipes and && ampersands"
        assert error is None
        assert not body_file.exists()

    def test_missing_file_returns_error(self, tmp_path):
        body_file = tmp_path / "nonexistent.md"

        body, error = issue_mod.read_body_file(str(body_file))

        assert body is None
        assert "Could not read body file" in error

    def test_empty_file_returns_empty_string(self, tmp_path):
        body_file = tmp_path / ".flow-issue-body"
        body_file.write_text("")

        body, error = issue_mod.read_body_file(str(body_file))

        assert body == ""
        assert error is None
        assert not body_file.exists()

    def test_rich_markdown_preserved(self, tmp_path):
        body_file = tmp_path / ".flow-issue-body"
        content = "## Summary\n\n| Column | Value |\n|--------|-------|\n| A | B |\n"
        body_file.write_text(content)

        body, error = issue_mod.read_body_file(str(body_file))

        assert body == content
        assert error is None

    def test_delete_failure_still_returns_body(self, tmp_path):
        body_file = tmp_path / ".flow-issue-body"
        body_file.write_text("Body text")

        with patch.object(issue_mod.os, "remove", side_effect=OSError("permission denied")):
            body, error = issue_mod.read_body_file(str(body_file))

        assert body == "Body text"
        assert error is None


class TestCreateIssue:
    """Tests for the create_issue function."""

    def test_happy_path_with_all_args(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/42\n")):
            result, error = issue_mod.create_issue(
                "owner/repo", "Test title", label="bug", body="Test body",
            )

        assert result["url"] == "https://github.com/owner/repo/issues/42"
        assert result["number"] == 42
        assert error is None

    def test_happy_path_minimal_args(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/1\n")):
            result, error = issue_mod.create_issue("owner/repo", "Title only")

        assert result["url"] == "https://github.com/owner/repo/issues/1"
        assert result["number"] == 1
        assert error is None

    def test_label_only_no_body(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/5\n")):
            result, error = issue_mod.create_issue(
                "owner/repo", "With label", label="enhancement",
            )

        assert result["url"] == "https://github.com/owner/repo/issues/5"
        assert result["number"] == 5
        assert error is None

    def test_body_only_no_label(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/7\n")):
            result, error = issue_mod.create_issue(
                "owner/repo", "With body", body="Details here",
            )

        assert result["url"] == "https://github.com/owner/repo/issues/7"
        assert result["number"] == 7
        assert error is None

    def test_gh_failure_stderr(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="HTTP 422: Validation Failed",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            result, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert result is None
        assert error == "HTTP 422: Validation Failed"

    def test_gh_failure_stdout_fallback(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="Something went wrong",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            result, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert result is None
        assert error == "Something went wrong"

    def test_gh_failure_unknown(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            result, error = issue_mod.create_issue("owner/repo", "Bad title")

        assert result is None
        assert error == "Unknown error"

    def test_timeout_returns_error(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            result, error = issue_mod.create_issue("owner/repo", "Test")

        assert result is None
        assert "timed out" in error.lower()


def _make_subprocess_router(create_stdout, api_stdout="99999\n",
                            create_rc=0, api_rc=0,
                            create_stderr="", api_stderr=""):
    """Build a subprocess.run side_effect routing gh issue vs gh api."""
    create_result = subprocess.CompletedProcess(
        args=[], returncode=create_rc,
        stdout=create_stdout, stderr=create_stderr,
    )
    api_result = subprocess.CompletedProcess(
        args=[], returncode=api_rc,
        stdout=api_stdout, stderr=api_stderr,
    )

    def side_effect(cmd, **kwargs):
        if cmd[1] == "issue":
            return create_result
        if cmd[1] == "api":
            return api_result
        raise ValueError(f"Unexpected command: {cmd}")
    return side_effect


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_success_with_body_file(self, capsys, tmp_path):
        body_file = tmp_path / ".flow-issue-body"
        body_file.write_text("Body from file")
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/10\n")), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test", "--label", "bug",
                                "--body-file", str(body_file)]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/owner/repo/issues/10"
        assert output["number"] == 10
        assert not body_file.exists()

    def test_main_success_no_body(self, capsys):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/owner/repo/issues/11\n")), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "No body"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["number"] == 11

    def test_main_body_file_missing(self, capsys, tmp_path):
        missing = tmp_path / "gone.md"
        with patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test",
                                "--body-file", str(missing)]), \
             pytest.raises(SystemExit, match="1"):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"
        assert "Could not read body file" in output["message"]

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
        with patch.object(issue_mod, "detect_repo", return_value="detected/repo"), \
             patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/detected/repo/issues/99\n")), \
             patch("sys.argv", ["issue.py", "--title", "Auto detected"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/detected/repo/issues/99"

    def test_main_explicit_repo_overrides(self, capsys):
        with patch.object(issue_mod, "detect_repo") as mock_detect, \
             patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/explicit/repo/issues/1\n")), \
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

    def test_main_uses_repo_from_state_file(self, capsys, tmp_path):
        """--state-file reads repo from state before falling back to detect_repo."""
        state_file = tmp_path / "state.json"
        state_file.write_text(json.dumps({"repo": "cached/repo", "branch": "test"}))
        with patch.object(issue_mod, "detect_repo") as mock_detect, \
             patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/cached/repo/issues/55\n")), \
             patch("sys.argv", ["issue.py", "--title", "From state",
                                "--state-file", str(state_file)]):
            issue_mod.main()

        mock_detect.assert_not_called()
        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/cached/repo/issues/55"

    def test_main_state_file_corrupt_falls_back(self, capsys, tmp_path):
        """--state-file with corrupt JSON falls back to detect_repo."""
        state_file = tmp_path / "bad.json"
        state_file.write_text("{corrupt")
        with patch.object(issue_mod, "detect_repo", return_value="detected/repo"), \
             patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/detected/repo/issues/88\n")), \
             patch("sys.argv", ["issue.py", "--title", "Corrupt state",
                                "--state-file", str(state_file)]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"

    def test_main_state_file_no_repo_falls_back(self, capsys, tmp_path):
        """--state-file with no repo key falls back to detect_repo."""
        state_file = tmp_path / "state.json"
        state_file.write_text(json.dumps({"branch": "test"}))
        with patch.object(issue_mod, "detect_repo", return_value="detected/repo"), \
             patch.object(issue_mod.subprocess, "run",
                          side_effect=_make_subprocess_router(
                              "https://github.com/detected/repo/issues/77\n")), \
             patch("sys.argv", ["issue.py", "--title", "Fallback",
                                "--state-file", str(state_file)]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"


class TestParseIssueNumber:
    """Tests for parse_issue_number — URL to issue number extraction."""

    def test_extracts_number_from_standard_url(self):
        url = "https://github.com/owner/repo/issues/42"
        assert issue_mod.parse_issue_number(url) == 42

    def test_extracts_number_from_url_with_trailing_newline(self):
        url = "https://github.com/owner/repo/issues/123\n"
        assert issue_mod.parse_issue_number(url.strip()) == 123

    def test_extracts_large_number(self):
        url = "https://github.com/owner/repo/issues/99999"
        assert issue_mod.parse_issue_number(url) == 99999

    def test_returns_none_for_invalid_url(self):
        assert issue_mod.parse_issue_number("not a url") is None

    def test_returns_none_for_empty_string(self):
        assert issue_mod.parse_issue_number("") is None

    def test_returns_none_for_pull_request_url(self):
        url = "https://github.com/owner/repo/pull/42"
        assert issue_mod.parse_issue_number(url) is None


class TestFetchDatabaseId:
    """Tests for fetch_database_id — REST API database ID lookup."""

    def test_happy_path_returns_integer_id(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="123456789\n", stderr="",
        )
        with patch.object(issue_mod.subprocess, "run",
                          return_value=fake_result) as mock_run:
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id == 123456789
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "api", "repos/owner/repo/issues/42", "--jq", ".id"],
            capture_output=True, text=True, timeout=30,
        )

    def test_gh_api_failure_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Not Found",
        )
        with patch.object(issue_mod.subprocess, "run",
                          return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 999)

        assert db_id is None
        assert "Not Found" in error

    def test_timeout_returns_error(self):
        with patch.object(issue_mod.subprocess, "run",
                          side_effect=subprocess.TimeoutExpired(
                              cmd="gh", timeout=30)):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert "timed out" in error.lower()

    def test_invalid_output_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="not_a_number\n", stderr="",
        )
        with patch.object(issue_mod.subprocess, "run",
                          return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert "Invalid" in error

    def test_empty_output_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="\n", stderr="",
        )
        with patch.object(issue_mod.subprocess, "run",
                          return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert error is not None


class TestCreateIssueEnhanced:
    """Tests for create_issue returning dict with number and id."""

    def test_returns_dict_with_url_number_id(self):
        create_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/42\n",
            stderr="",
        )
        api_result = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="123456789\n", stderr="",
        )

        def side_effect(cmd, **kwargs):
            if cmd[1] == "issue":
                return create_result
            if cmd[1] == "api":
                return api_result
            raise ValueError(f"Unexpected command: {cmd}")

        with patch.object(issue_mod.subprocess, "run",
                          side_effect=side_effect):
            result, error = issue_mod.create_issue(
                "owner/repo", "Test title",
            )

        assert error is None
        assert result["url"] == "https://github.com/owner/repo/issues/42"
        assert result["number"] == 42
        assert result["id"] == 123456789

    def test_id_is_none_when_api_fails(self):
        create_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/42\n",
            stderr="",
        )
        api_result = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Not Found",
        )

        def side_effect(cmd, **kwargs):
            if cmd[1] == "issue":
                return create_result
            if cmd[1] == "api":
                return api_result
            raise ValueError(f"Unexpected command: {cmd}")

        with patch.object(issue_mod.subprocess, "run",
                          side_effect=side_effect):
            result, error = issue_mod.create_issue(
                "owner/repo", "Test title",
            )

        assert error is None
        assert result["url"] == "https://github.com/owner/repo/issues/42"
        assert result["number"] == 42
        assert result["id"] is None

    def test_gh_issue_create_failure_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Auth required",
        )
        with patch.object(issue_mod.subprocess, "run",
                          return_value=fake_result):
            result, error = issue_mod.create_issue(
                "owner/repo", "Test title",
            )

        assert result is None
        assert error == "Auth required"


class TestMainEnhanced:
    """Tests for main() output including number and id fields."""

    def test_main_outputs_number_and_id(self, capsys):
        create_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/42\n",
            stderr="",
        )
        api_result = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="123456789\n", stderr="",
        )

        def side_effect(cmd, **kwargs):
            if cmd[1] == "issue":
                return create_result
            if cmd[1] == "api":
                return api_result
            raise ValueError(f"Unexpected command: {cmd}")

        with patch.object(issue_mod.subprocess, "run",
                          side_effect=side_effect), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["url"] == "https://github.com/owner/repo/issues/42"
        assert output["number"] == 42
        assert output["id"] == 123456789

    def test_main_id_null_on_api_failure(self, capsys):
        create_result = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout="https://github.com/owner/repo/issues/42\n",
            stderr="",
        )
        api_result = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Not Found",
        )

        def side_effect(cmd, **kwargs):
            if cmd[1] == "issue":
                return create_result
            if cmd[1] == "api":
                return api_result
            raise ValueError(f"Unexpected command: {cmd}")

        with patch.object(issue_mod.subprocess, "run",
                          side_effect=side_effect), \
             patch("sys.argv", ["issue.py", "--repo", "owner/repo",
                                "--title", "Test"]):
            issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["number"] == 42
        assert output["id"] is None
