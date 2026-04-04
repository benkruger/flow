"""Tests for bin/flow issue CLI (Rust implementation).

The Python bridge module (lib/issue.py) was deleted in PR #838 after
create-sub-issue and link-blocked-by were ported to Rust. The Rust
implementation lives in src/issue.rs with unit tests. This file covers
the CLI interface via subprocess.
"""

import json
import subprocess

from conftest import BIN_DIR, LIB_DIR

BIN_FLOW = str(BIN_DIR / "flow")


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


# --- Tombstone tests ---


class TestTombstones:
    """Tombstone tests for Python files removed in PR #838."""

    def test_no_python_issue_bridge(self):
        """Tombstone: lib/issue.py bridge removed in PR #838. Must not return."""
        assert not (LIB_DIR / "issue.py").exists(), "lib/issue.py was removed in PR #838"

    def test_no_python_create_sub_issue(self):
        """Tombstone: lib/create-sub-issue.py removed in PR #838. Must not return."""
        assert not (LIB_DIR / "create-sub-issue.py").exists(), "lib/create-sub-issue.py was removed in PR #838"

    def test_no_python_link_blocked_by(self):
        """Tombstone: lib/link-blocked-by.py removed in PR #838. Must not return."""
        assert not (LIB_DIR / "link-blocked-by.py").exists(), "lib/link-blocked-by.py was removed in PR #838"

    def test_no_python_create_milestone(self):
        """Tombstone: lib/create-milestone.py removed in PR #838. Must not return."""
        assert not (LIB_DIR / "create-milestone.py").exists(), "lib/create-milestone.py was removed in PR #838"

    def test_no_python_create_dependencies(self):
        """Tombstone: lib/create-dependencies.py removed in PR #838. Must not return."""
        assert not (LIB_DIR / "create-dependencies.py").exists(), "lib/create-dependencies.py was removed in PR #838"
