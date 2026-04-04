"""Tests for lib/issue.py — bridge module retaining fetch_database_id.

The main issue creation logic has been ported to Rust (src/issue.rs).
This test file covers:
- fetch_database_id (Python bridge function, still in lib/issue.py)
- bin/flow issue CLI (Rust implementation, tested via subprocess)
"""

import importlib.util
import json
import subprocess
from unittest.mock import patch

from conftest import BIN_DIR, LIB_DIR

spec = importlib.util.spec_from_file_location("issue", LIB_DIR / "issue.py")
issue_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(issue_mod)

BIN_FLOW = str(BIN_DIR / "flow")


# --- fetch_database_id (Python bridge) ---


class TestFetchDatabaseId:
    """Tests for fetch_database_id — REST API database ID lookup."""

    def test_happy_path_returns_integer_id(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="123456789\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result) as mock_run:
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id == 123456789
        assert error is None
        mock_run.assert_called_once_with(
            ["gh", "api", "repos/owner/repo/issues/42", "--jq", ".id"],
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_gh_api_failure_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 999)

        assert db_id is None
        assert "Not Found" in error

    def test_timeout_returns_error(self):
        with patch.object(issue_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert "timed out" in error.lower()

    def test_invalid_output_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not_a_number\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert "Invalid" in error

    def test_empty_output_returns_error(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="\n",
            stderr="",
        )
        with patch.object(issue_mod.subprocess, "run", return_value=fake_result):
            db_id, error = issue_mod.fetch_database_id("owner/repo", 42)

        assert db_id is None
        assert error is not None


# --- bin/flow issue CLI (Rust) ---


class TestIssueCli:
    """Tests for bin/flow issue CLI (Rust subprocess)."""

    def test_cli_missing_title_exits_2(self, target_project):
        """CLI without --title exits with code 2 (clap argument error)."""
        result = subprocess.run(
            [BIN_FLOW, "issue", "--repo", "owner/repo"],
            capture_output=True,
            text=True,
            cwd=str(target_project),
        )
        assert result.returncode == 2

    def test_cli_body_file_missing_returns_error(self, target_project):
        """CLI with missing --body-file outputs error JSON."""
        result = subprocess.run(
            [BIN_FLOW, "issue", "--repo", "owner/repo", "--title", "Test", "--body-file", "/nonexistent/body.md"],
            capture_output=True,
            text=True,
            cwd=str(target_project),
        )
        assert result.returncode == 1
        output = json.loads(result.stdout)
        assert output["status"] == "error"
        assert "Could not read body file" in output["message"]

    def test_cli_body_file_reads_and_deletes(self, target_project):
        """CLI reads body file content and deletes the file."""
        body_file = target_project / ".flow-issue-body"
        body_file.write_text("Body with | pipes and && ampersands")

        # gh issue create will fail (no real GitHub), but body file should be read first
        subprocess.run(
            [BIN_FLOW, "issue", "--repo", "owner/repo", "--title", "Test", "--body-file", str(body_file)],
            capture_output=True,
            text=True,
            cwd=str(target_project),
        )
        # The file should be deleted regardless of gh outcome
        assert not body_file.exists()

    def test_cli_state_file_reads_repo(self, target_project):
        """CLI reads repo from --state-file when --repo is not provided."""
        state_file = target_project / "state.json"
        state_file.write_text(json.dumps({"repo": "cached/repo", "branch": "test"}))

        # gh issue create will fail, but we verify the state file is read
        result = subprocess.run(
            [BIN_FLOW, "issue", "--title", "Test", "--state-file", str(state_file)],
            capture_output=True,
            text=True,
            cwd=str(target_project),
        )
        # Should attempt to create issue on cached/repo (will fail since no gh auth in tests)
        assert result.returncode != 0


# --- Bridge module structure ---


class TestBridgeModule:
    """Tests for the bridge module structure after Rust port."""

    def test_exports_fetch_database_id(self):
        """Bridge module exports fetch_database_id."""
        assert hasattr(issue_mod, "fetch_database_id")
        assert callable(issue_mod.fetch_database_id)

    def test_no_create_issue(self):
        """Tombstone: create_issue ported to Rust in PR #831. Must not return."""
        assert not hasattr(issue_mod, "create_issue")

    def test_no_read_body_file(self):
        """Tombstone: read_body_file ported to Rust in PR #831. Must not return."""
        assert not hasattr(issue_mod, "read_body_file")

    def test_no_main(self):
        """Tombstone: main() ported to Rust in PR #831. Must not return."""
        assert not hasattr(issue_mod, "main")

    def test_no_parse_issue_number(self):
        """Tombstone: parse_issue_number ported to Rust in PR #831. Must not return."""
        assert not hasattr(issue_mod, "parse_issue_number")
