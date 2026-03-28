"""Tests for lib/link-blocked-by.py — GitHub blocked-by dependency linking."""

import importlib.util
import json
import subprocess
from unittest.mock import patch

import pytest
from conftest import LIB_DIR

spec = importlib.util.spec_from_file_location("link_blocked_by", LIB_DIR / "link-blocked-by.py")
blocked_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(blocked_mod)


def _make_api_router(
    blocked_id=100,
    blocking_id=200,
    link_ok=True,
    blocked_fail=False,
    blocking_fail=False,
    body_text="Existing body",
    body_fetch_fail=False,
    body_update_fail=False,
    body_update_timeout=False,
):
    """Build a side_effect routing gh api calls."""
    call_count = {"n": 0}

    def side_effect(cmd, **kwargs):
        call_count["n"] += 1
        url = cmd[2]

        # Link creation
        if "/dependencies/blocked_by" in url:
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

        # Body fetch (--jq .body)
        if "--jq" in cmd and ".body" in cmd:
            if body_fetch_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Body fetch failed",
                )
            stdout = "null\n" if body_text is None else f"{body_text}\n"
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=stdout,
                stderr="",
            )

        # Body update (--method PATCH)
        if "--method" in cmd and "PATCH" in cmd:
            if body_update_timeout:
                raise subprocess.TimeoutExpired(cmd="gh", timeout=30)
            if body_update_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Body update failed",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="{}",
                stderr="",
            )

        # ID resolution (call-count based)
        if call_count["n"] == 1:
            if blocked_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Blocked not found",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{blocked_id}\n",
                stderr="",
            )
        else:
            if blocking_fail:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=1,
                    stdout="",
                    stderr="Blocking not found",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{blocking_id}\n",
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
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            db_id, error = blocked_mod.resolve_database_id("o/r", 42)

        assert db_id == 123456
        assert error is None

    def test_timeout(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            db_id, error = blocked_mod.resolve_database_id("o/r", 42)

        assert db_id is None
        assert "timed out" in error.lower()

    def test_invalid_output(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not_a_number\n",
            stderr="",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            db_id, error = blocked_mod.resolve_database_id("o/r", 42)

        assert db_id is None
        assert "Invalid" in error


class TestLinkBlockedBy:
    """Tests for the link_blocked_by function."""

    def test_happy_path(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(100, 200)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert result["blocking"] == 20

    def test_blocked_id_resolution_fails(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(blocked_fail=True)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert result is None
        assert "blocked" in error.lower()

    def test_blocking_id_resolution_fails(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(blocking_fail=True)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert result is None
        assert "blocking" in error.lower()

    def test_link_creation_fails(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(link_ok=False)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert result is None
        assert "Link failed" in error

    def test_uses_integer_flag_for_issue_id(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(100, 200)) as mock_run:
            blocked_mod.link_blocked_by("o/r", 10, 20)

        # Find the API creation call (the one hitting /dependencies/blocked_by)
        link_calls = [c for c in mock_run.call_args_list if "/dependencies/blocked_by" in str(c)]
        assert len(link_calls) == 1, f"Expected 1 link call, got {len(link_calls)}"
        cmd = link_calls[0].args[0]
        # The flag before issue_id= must be -F (integer type), not -f (string type)
        issue_id_idx = next(i for i, arg in enumerate(cmd) if arg.startswith("issue_id="))
        assert cmd[issue_id_idx - 1] == "-F", f"Expected -F before issue_id, got {cmd[issue_id_idx - 1]}"

    def test_link_creation_timeout(self):
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            url = cmd[2]
            if "/dependencies/blocked_by" in url:
                raise subprocess.TimeoutExpired(cmd="gh", timeout=30)
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout=f"{call_count['n'] * 100}\n",
                stderr="",
            )

        with patch.object(blocked_mod.subprocess, "run", side_effect=side_effect):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert result is None
        assert "timed out" in error.lower()

    def test_body_update_happy_path(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(body_text="Some body")):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert result["blocking"] == 20
        assert "body_warning" not in result

    def test_body_update_skips_duplicate(self):
        # Body already has the reference — no update needed
        with patch.object(
            blocked_mod.subprocess, "run", side_effect=_make_api_router(body_text="Issue.\n\n## Blocked by\n\n- #20\n")
        ):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert "body_warning" not in result

    def test_body_fetch_failure_returns_warning(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(body_fetch_fail=True)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert "body_warning" in result

    def test_body_update_failure_returns_warning(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(body_update_fail=True)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert "body_warning" in result

    def test_body_update_timeout_returns_warning(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(body_update_timeout=True)):
            result, error = blocked_mod.link_blocked_by("o/r", 10, 20)

        assert error is None
        assert result["blocked"] == 10
        assert "body_warning" in result


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_success(self, capsys):
        with (
            patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(100, 200)),
            patch(
                "sys.argv", ["link-blocked-by.py", "--repo", "o/r", "--blocked-number", "10", "--blocking-number", "20"]
            ),
        ):
            blocked_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["blocked"] == 10
        assert output["blocking"] == 20

    def test_main_failure(self, capsys):
        with (
            patch.object(blocked_mod.subprocess, "run", side_effect=_make_api_router(blocked_fail=True)),
            patch(
                "sys.argv", ["link-blocked-by.py", "--repo", "o/r", "--blocked-number", "10", "--blocking-number", "20"]
            ),
            pytest.raises(SystemExit, match="1"),
        ):
            blocked_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "error"

    def test_main_missing_required_args(self):
        with patch("sys.argv", ["link-blocked-by.py", "--repo", "o/r"]), pytest.raises(SystemExit, match="2"):
            blocked_mod.main()


class TestBuildBlockedBySection:
    """Tests for the build_blocked_by_section function."""

    def test_no_existing_section(self):
        body = "Some issue description.\n\n## Context\n\nMore info here."
        result = blocked_mod.build_blocked_by_section(body, 42)
        assert "## Blocked by" in result
        assert "- #42" in result
        # Original content preserved
        assert body.rstrip() in result

    def test_existing_section_appends(self):
        body = "Description.\n\n## Blocked by\n\n- #10\n"
        result = blocked_mod.build_blocked_by_section(body, 42)
        assert "- #10" in result
        assert "- #42" in result

    def test_duplicate_prevention(self):
        body = "Description.\n\n## Blocked by\n\n- #42\n"
        result = blocked_mod.build_blocked_by_section(body, 42)
        assert result == body

    def test_empty_body(self):
        result = blocked_mod.build_blocked_by_section("", 42)
        assert "## Blocked by" in result
        assert "- #42" in result

    def test_none_body(self):
        result = blocked_mod.build_blocked_by_section(None, 42)
        assert "## Blocked by" in result
        assert "- #42" in result

    def test_section_not_last(self):
        body = "Description.\n\n## Blocked by\n\n- #10\n\n## Notes\n\nSome notes."
        result = blocked_mod.build_blocked_by_section(body, 42)
        assert "- #10" in result
        assert "- #42" in result
        # Notes section preserved after
        assert "## Notes" in result
        assert "Some notes." in result


class TestFetchIssueBody:
    """Tests for the fetch_issue_body function."""

    def test_happy_path(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="Issue body content here\n",
            stderr="",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            body, error = blocked_mod.fetch_issue_body("o/r", 42)

        assert body == "Issue body content here"
        assert error is None

    def test_empty_body(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="\n",
            stderr="",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            body, error = blocked_mod.fetch_issue_body("o/r", 42)

        assert body == ""
        assert error is None

    def test_null_body(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="null\n",
            stderr="",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            body, error = blocked_mod.fetch_issue_body("o/r", 42)

        assert body is None
        assert error is None

    def test_api_failure(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            body, error = blocked_mod.fetch_issue_body("o/r", 42)

        assert body is None
        assert "Not Found" in error

    def test_timeout(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            body, error = blocked_mod.fetch_issue_body("o/r", 42)

        assert body is None
        assert "timed out" in error.lower()


class TestUpdateIssueBody:
    """Tests for the update_issue_body function."""

    def test_happy_path(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="{}",
            stderr="",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            error = blocked_mod.update_issue_body("o/r", 42, "new body")

        assert error is None

    def test_api_failure(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Validation Failed",
        )
        with patch.object(blocked_mod.subprocess, "run", return_value=fake_result):
            error = blocked_mod.update_issue_body("o/r", 42, "new body")

        assert "Validation Failed" in error

    def test_timeout(self):
        with patch.object(blocked_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            error = blocked_mod.update_issue_body("o/r", 42, "new body")

        assert "timed out" in error.lower()
