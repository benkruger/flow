"""Tests for lib/auto-close-parent.py — auto-close parent issue and milestone."""

import importlib.util
import json
import subprocess
from unittest.mock import patch

import pytest
from conftest import LIB_DIR

spec = importlib.util.spec_from_file_location("auto_close_parent", LIB_DIR / "auto-close-parent.py")
auto_close_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(auto_close_mod)


class TestFetchIssueFields:
    """Tests for _fetch_issue_fields — single API call for both fields."""

    def test_returns_both_fields(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"parent_issue": {"number": 10}, "milestone": {"number": 3}}\n',
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent == 10
        assert milestone == 3

    def test_no_parent_no_milestone(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="{}\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent is None
        assert milestone is None

    def test_api_failure_returns_none_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent is None
        assert milestone is None

    def test_invalid_json_returns_none_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not json\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent is None
        assert milestone is None

    def test_parent_not_dict_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"parent_issue": "not_a_dict", "milestone": {"number": 3}}\n',
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent is None
        assert milestone == 3

    def test_milestone_number_not_int_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout='{"parent_issue": {"number": 10}, "milestone": {"number": "not_int"}}\n',
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            parent, milestone = auto_close_mod._fetch_issue_fields("o/r", 5)

        assert parent == 10
        assert milestone is None


class TestCheckParent:
    """Tests for check_parent_closed."""

    def test_all_siblings_closed_closes_parent(self):
        """When all sub-issues of a parent are closed, parent is closed."""
        calls = []

        def side_effect(cmd, **kwargs):
            calls.append(cmd)
            url = cmd[2]
            if url.endswith("/issues/5") and "--jq" in cmd:
                # Get parent_issue.number from the child's parent_issue field
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="10\n",
                    stderr="",
                )
            if "/issues/10/sub_issues" in url:
                # All sub-issues closed
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='[{"number": 5, "state": "closed"}, {"number": 6, "state": "closed"}]\n',
                    stderr="",
                )
            if cmd[1] == "issue" and cmd[2] == "close":
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is True

    def test_some_siblings_open_does_not_close(self):
        """When some sub-issues are still open, parent is not closed."""

        def side_effect(cmd, **kwargs):
            url = cmd[2]
            if url.endswith("/issues/5") and "--jq" in cmd:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="10\n",
                    stderr="",
                )
            if "/issues/10/sub_issues" in url:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='[{"number": 5, "state": "closed"}, {"number": 6, "state": "open"}]\n',
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_no_parent_returns_false(self):
        """When issue has no parent, returns False."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_invalid_parent_number_returns_false(self):
        """When parent number is not a valid integer, returns False."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not_a_number\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_sub_issues_api_failure_returns_false(self):
        """When sub-issues fetch fails, returns False."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            if call_count["n"] == 1:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="10\n",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=1,
                stdout="",
                stderr="Error",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_invalid_sub_issues_json_returns_false(self):
        """When sub-issues response is not valid JSON, returns False."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            if call_count["n"] == 1:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="10\n",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="not json\n",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_empty_sub_issues_returns_false(self):
        """When parent has no sub-issues, returns False."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            if call_count["n"] == 1:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="10\n",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="[]\n",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_api_failure_returns_false(self):
        """When gh api fails, returns False (best-effort)."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False

    def test_timeout_returns_false(self):
        """When gh api times out, returns False (best-effort)."""
        with patch.object(
            auto_close_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)
        ):
            closed = auto_close_mod.check_parent_closed("o/r", 5)

        assert closed is False


class TestCheckMilestone:
    """Tests for check_milestone_closed."""

    def test_all_issues_closed_closes_milestone(self):
        """When all milestone issues are closed, milestone is closed."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            url = cmd[2]
            if url.endswith("/issues/5") and "--jq" in cmd:
                # Issue has milestone number 3
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="3\n",
                    stderr="",
                )
            if "/milestones/3" in url and "--method" not in cmd:
                # Milestone check: open_issues = 0
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='{"open_issues": 0, "closed_issues": 5}\n',
                    stderr="",
                )
            if "/milestones/3" in url and "--method" in cmd:
                # Close milestone
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="{}",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is True

    def test_open_issues_remain_does_not_close(self):
        """When milestone has open issues, does not close."""

        def side_effect(cmd, **kwargs):
            url = cmd[2]
            if url.endswith("/issues/5") and "--jq" in cmd:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="3\n",
                    stderr="",
                )
            if "/milestones/3" in url:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='{"open_issues": 2, "closed_issues": 3}\n',
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_no_milestone_returns_false(self):
        """When issue has no milestone, returns False."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="null\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_invalid_milestone_number_returns_false(self):
        """When milestone number is not a valid integer, returns False."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not_a_number\n",
            stderr="",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_milestone_api_failure_returns_false(self):
        """When milestone check fails, returns False."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            if call_count["n"] == 1:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="3\n",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=1,
                stdout="",
                stderr="Error",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_milestone_invalid_json_returns_false(self):
        """When milestone response is not valid JSON, returns False."""
        call_count = {"n": 0}

        def side_effect(cmd, **kwargs):
            call_count["n"] += 1
            if call_count["n"] == 1:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="3\n",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="not json\n",
                stderr="",
            )

        with patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_api_failure_returns_false(self):
        """When gh api fails, returns False (best-effort)."""
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Not Found",
        )
        with patch.object(auto_close_mod.subprocess, "run", return_value=fake_result):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False

    def test_timeout_returns_false(self):
        """When gh api times out, returns False (best-effort)."""
        with patch.object(
            auto_close_mod.subprocess, "run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)
        ):
            closed = auto_close_mod.check_milestone_closed("o/r", 5)

        assert closed is False


class TestMain:
    """Tests for the main() CLI entry point."""

    def test_main_both_closed(self, capsys):
        def side_effect(cmd, **kwargs):
            url = cmd[2] if len(cmd) > 2 else ""
            # Parent check
            if url.endswith("/issues/5") and "--jq" in cmd:
                jq_expr = cmd[cmd.index("--jq") + 1]
                if "parent_issue" in jq_expr:
                    return subprocess.CompletedProcess(
                        args=cmd,
                        returncode=0,
                        stdout="10\n",
                        stderr="",
                    )
                if "milestone" in jq_expr:
                    return subprocess.CompletedProcess(
                        args=cmd,
                        returncode=0,
                        stdout="3\n",
                        stderr="",
                    )
            if "/issues/10/sub_issues" in url:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='[{"number": 5, "state": "closed"}]\n',
                    stderr="",
                )
            if "/milestones/3" in url and "--method" not in cmd:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout='{"open_issues": 0, "closed_issues": 1}\n',
                    stderr="",
                )
            # Close operations
            if cmd[1] == "issue" and cmd[2] == "close":
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="",
                    stderr="",
                )
            if "--method" in cmd:
                return subprocess.CompletedProcess(
                    args=cmd,
                    returncode=0,
                    stdout="{}",
                    stderr="",
                )
            return subprocess.CompletedProcess(
                args=cmd,
                returncode=0,
                stdout="",
                stderr="",
            )

        with (
            patch.object(auto_close_mod.subprocess, "run", side_effect=side_effect),
            patch("sys.argv", ["auto-close-parent.py", "--repo", "o/r", "--issue-number", "5"]),
        ):
            auto_close_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["parent_closed"] is True
        assert output["milestone_closed"] is True

    def test_main_neither_closed(self, capsys):
        fake_result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="\n",
            stderr="",
        )
        with (
            patch.object(auto_close_mod.subprocess, "run", return_value=fake_result),
            patch("sys.argv", ["auto-close-parent.py", "--repo", "o/r", "--issue-number", "5"]),
        ):
            auto_close_mod.main()

        output = json.loads(capsys.readouterr().out)
        assert output["status"] == "ok"
        assert output["parent_closed"] is False
        assert output["milestone_closed"] is False

    def test_main_missing_required_args(self):
        with patch("sys.argv", ["auto-close-parent.py", "--repo", "o/r"]), pytest.raises(SystemExit, match="2"):
            auto_close_mod.main()
