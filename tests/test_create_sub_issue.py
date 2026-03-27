"""Tests for lib/create-sub-issue.py — GitHub sub-issue relationship creation."""

import importlib.util
import json
import subprocess
from unittest.mock import patch

import pytest
from conftest import LIB_DIR

spec = importlib.util.spec_from_file_location("create_sub_issue", LIB_DIR / "create-sub-issue.py")
sub_issue_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sub_issue_mod)


def _make_api_router(parent_id=100, child_id=200, link_ok=True, parent_fail=False, child_fail=False):
    """Build a side_effect routing gh api calls by URL pattern."""
    call_count = {"n": 0}

    def side_effect(cmd, **kwargs):
        call_count["n"] += 1
        url = cmd[2]  # gh api <url>
        if "/sub_issues" in url:
            # Link call
            if not link_ok:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Link failed",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="{}",
                stderr="",
            )
        # ID resolution call — determine parent vs child by order
        if call_count["n"] == 1:
            # First call = parent
            if parent_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Parent not found",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{parent_id}\n",
                stderr="",
            )
        else:
            # Second call = child
            if child_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Child not found",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{child_id}\n",
                stderr="",
            )

    return side_effect


class TestResolveId:
    """Tests for resolve_database_id helper."""

    def test_happy_path(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="123456\n",
            stderr="",
        )
        with patch.object(sub_issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = sub_issue_mod.resolve_database_id("o/r", 42)

        assert db_id == 123456
        assert error is None

    def test_api_failure(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(sub_issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = sub_issue_mod.resolve_database_id("o/r", 999)

        assert db_id is None
        assert "Not Found" in error

    def test_timeout(self):
        with patch.object(sub_issue_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            db_id, error = sub_issue_mod.resolve_database_id("o/r", 42)

        assert db_id is None
        assert "timed out" in error.lower()

    def test_invalid_output(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not_a_number\n",
            stderr="",
        )
        with patch.object(sub_issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = sub_issue_mod.resolve_database_id("o/r", 42)

        assert db_id is None
        assert "Invalid" in error


class TestCreateSubIssue:
    """Tests for the create_sub_issue function."""

    def test_happy_path(self):
        with patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(100, 200)):
            result, error = sub_issue_mod.create_sub_issue("o/r", 1, 2)

        assert error is None
        assert result["parent"] == 1
        assert result["child"] == 2

    def test_parent_id_resolution_fails(self):
        with patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(parent_fail=True)):
            result, error = sub_issue_mod.create_sub_issue("o/r", 1, 2)

        assert result is None
        assert "parent" in error.lower()

    def test_child_id_resolution_fails(self):
        with patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(child_fail=True)):
            result, error = sub_issue_mod.create_sub_issue("o/r", 1, 2)

        assert result is None
        assert "child" in error.lower()

    def test_link_creation_fails(self):
        with patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(link_ok=False)):
            result, error = sub_issue_mod.create_sub_issue("o/r", 1, 2)

        assert result is None
        assert "Link failed" in error

    def test_link_creation_timeout(self):
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            url = cmd[2]
            if "/sub_issues" in url:
                raise subprocess.TimeoutExpired(cmd="gh", timeout=30)
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{call_count['n'] * 100}\n",
                stderr="",
            )

        with patch.object(sub_issue_mod.subprocess, "run", side_effect=side_effect):
            result, error = sub_issue_mod.create_sub_issue("o/r", 1, 2)

        assert result is None
        assert "timed out" in error.lower()


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_success(self, capsys):
        with (
            patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(100, 200)),
            patch("sys.argv", ["create-sub-issue.py", "--repo", "o/r", "--parent-number", "1", "--child-number", "2"]),
        ):
            sub_issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["parent"] == 1
        assert output["child"] == 2

    def test_main_failure(self, capsys):
        with (
            patch.object(sub_issue_mod.subprocess, "run", side_effect=_make_api_router(parent_fail=True)),
            patch("sys.argv", ["create-sub-issue.py", "--repo", "o/r", "--parent-number", "1", "--child-number", "2"]),
            pytest.raises(SystemExit, match="1"),
        ):
            sub_issue_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"

    def test_main_missing_required_args(self):
        with patch("sys.argv", ["create-sub-issue.py", "--repo", "o/r"]), pytest.raises(SystemExit, match="2"):
            sub_issue_mod.main()
