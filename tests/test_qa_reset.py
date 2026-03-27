"""Tests for lib/qa-reset.py — reset QA repos to seed state."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("qa-reset")

REPO_ROOT = Path(__file__).resolve().parent.parent


# --- reset_git ---


def test_reset_git_runs_correct_commands():
    """reset_git() runs git reset --hard seed and force push."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="",
            stderr="",
        )
        result = _mod.reset_git("/tmp/repo")

    assert result["status"] == "ok"
    cmds = [c[0][0] for c in mock_run.call_args_list]
    # Should include reset --hard seed and push -f
    reset_cmd = [c for c in cmds if "reset" in c]
    assert len(reset_cmd) >= 1
    push_cmd = [c for c in cmds if "push" in c]
    assert len(push_cmd) >= 1


def test_reset_git_failure():
    """reset_git() returns error on git failure."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="fatal: not a repo",
        )
        result = _mod.reset_git("/tmp/repo")

    assert result["status"] == "error"


# --- close_prs ---


def test_close_prs_closes_all_open():
    """close_prs() lists and closes all open PRs."""
    pr_list_output = json.dumps(
        [
            {"number": 1},
            {"number": 2},
        ]
    )

    def side_effect(args, **kwargs):
        if "list" in args:
            return subprocess.CompletedProcess(
                args=args,
                returncode=0,
                stdout=pr_list_output,
                stderr="",
            )
        return subprocess.CompletedProcess(
            args=args,
            returncode=0,
            stdout="",
            stderr="",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.close_prs("owner/repo")

    assert result == 2


def test_close_prs_no_open():
    """close_prs() returns 0 when no PRs are open."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="[]",
            stderr="",
        )
        result = _mod.close_prs("owner/repo")

    assert result == 0


# --- delete_remote_branches ---


def test_delete_remote_branches():
    """delete_remote_branches() deletes all non-main branches."""
    branch_output = "  origin/main\n  origin/feature-1\n  origin/feature-2\n"

    def side_effect(args, **kwargs):
        if "branch" in args and "-r" in args:
            return subprocess.CompletedProcess(
                args=args,
                returncode=0,
                stdout=branch_output,
                stderr="",
            )
        return subprocess.CompletedProcess(
            args=args,
            returncode=0,
            stdout="",
            stderr="",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.delete_remote_branches("owner/repo", "/tmp/repo")

    assert result == 2


def test_delete_remote_branches_only_main():
    """delete_remote_branches() returns 0 when only main exists."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="  origin/main\n",
            stderr="",
        )
        result = _mod.delete_remote_branches("owner/repo", "/tmp/repo")

    assert result == 0


# --- reset_issues ---


def test_reset_issues_closes_and_recreates():
    """reset_issues() closes existing issues and creates from template."""
    issue_list = json.dumps([{"number": 1}, {"number": 2}])
    template = [
        {"title": "New issue", "body": "Body", "labels": []},
    ]

    call_count = {"close": 0, "create": 0}

    def side_effect(args, **kwargs):
        if "list" in args:
            return subprocess.CompletedProcess(
                args=args,
                returncode=0,
                stdout=issue_list,
                stderr="",
            )
        if "close" in args:
            call_count["close"] += 1
        if "create" in args:
            call_count["create"] += 1
        return subprocess.CompletedProcess(
            args=args,
            returncode=0,
            stdout="",
            stderr="",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.reset_issues("owner/repo", template)

    assert result == 1  # 1 issue created
    assert call_count["close"] == 2
    assert call_count["create"] == 1


def test_close_prs_gh_failure():
    """close_prs() returns 0 when gh pr list fails."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="error",
        )
        result = _mod.close_prs("owner/repo")

    assert result == 0


def test_delete_remote_branches_git_failure():
    """delete_remote_branches() returns 0 when git branch -r fails."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="error",
        )
        result = _mod.delete_remote_branches("owner/repo", "/tmp/repo")

    assert result == 0


def test_delete_remote_branches_empty_line():
    """delete_remote_branches() skips empty lines in output."""
    branch_output = "  origin/main\n\n  origin/feature-1\n"

    def side_effect(args, **kwargs):
        if "branch" in args and "-r" in args:
            return subprocess.CompletedProcess(
                args=args,
                returncode=0,
                stdout=branch_output,
                stderr="",
            )
        return subprocess.CompletedProcess(
            args=args,
            returncode=0,
            stdout="",
            stderr="",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.delete_remote_branches("owner/repo", "/tmp/repo")

    assert result == 1  # Only feature-1


# --- load_issue_template ---


def test_load_issue_template_success():
    """load_issue_template() decodes base64 content from GitHub API."""
    import base64

    content = json.dumps([{"title": "Test", "body": "Body", "labels": []}])
    encoded = base64.b64encode(content.encode()).decode()

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=encoded,
            stderr="",
        )
        result = _mod.load_issue_template("owner/repo")

    assert len(result) == 1
    assert result[0]["title"] == "Test"


def test_load_issue_template_failure():
    """load_issue_template() returns empty list on API failure."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="not found",
        )
        result = _mod.load_issue_template("owner/repo")

    assert result == []


def test_load_issue_template_corrupt():
    """load_issue_template() returns empty list on corrupt content."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="not-base64!!!",
            stderr="",
        )
        result = _mod.load_issue_template("owner/repo")

    assert result == []


# --- reset_issues with labels ---


def test_reset_issues_with_labels():
    """reset_issues() passes labels to gh issue create."""
    template = [
        {"title": "Bug", "body": "Fix it", "labels": ["bug", "urgent"]},
    ]
    calls = []

    def side_effect(args, **kwargs):
        calls.append(args)
        if "list" in args:
            return subprocess.CompletedProcess(
                args=args,
                returncode=0,
                stdout="[]",
                stderr="",
            )
        return subprocess.CompletedProcess(
            args=args,
            returncode=0,
            stdout="",
            stderr="",
        )

    with patch("subprocess.run", side_effect=side_effect):
        result = _mod.reset_issues("owner/repo", template)

    assert result == 1
    create_call = [c for c in calls if "create" in c][0]
    assert "--label" in create_call
    assert "bug" in create_call
    assert "urgent" in create_call


# --- clean_local ---


def test_clean_local_removes_flow_artifacts(tmp_path):
    """clean_local() removes .flow-states/, .flow.json, .claude/."""
    (tmp_path / ".flow-states").mkdir()
    (tmp_path / ".flow-states" / "test.json").write_text("{}")
    (tmp_path / ".flow.json").write_text("{}")
    (tmp_path / ".claude").mkdir()
    (tmp_path / ".claude" / "settings.json").write_text("{}")

    _mod.clean_local(str(tmp_path))

    assert not (tmp_path / ".flow-states").exists()
    assert not (tmp_path / ".flow.json").exists()
    assert not (tmp_path / ".claude").exists()


def test_clean_local_missing_artifacts(tmp_path):
    """clean_local() handles missing artifacts gracefully."""
    # No artifacts exist — should not raise
    _mod.clean_local(str(tmp_path))


# --- reset (main orchestrator) ---


def test_reset_full_workflow():
    """reset() calls all sub-functions in order."""
    with (
        patch.object(_mod, "reset_git") as mock_git,
        patch.object(_mod, "close_prs") as mock_prs,
        patch.object(_mod, "delete_remote_branches") as mock_branches,
        patch.object(_mod, "reset_issues") as mock_issues,
        patch.object(_mod, "clean_local") as mock_clean,
        patch.object(_mod, "load_issue_template") as mock_template,
    ):
        mock_git.return_value = {"status": "ok"}
        mock_prs.return_value = 2
        mock_branches.return_value = 3
        mock_template.return_value = [{"title": "T", "body": "B", "labels": []}]
        mock_issues.return_value = 1

        result = _mod.reset("owner/repo", local_path="/tmp/repo")

    assert result["status"] == "ok"
    assert result["prs_closed"] == 2
    assert result["branches_deleted"] == 3
    assert result["issues_reset"] == 1
    mock_clean.assert_called_once_with("/tmp/repo")


def test_reset_without_local_path():
    """reset() skips clean_local when no local_path provided."""
    with (
        patch.object(_mod, "reset_git") as mock_git,
        patch.object(_mod, "close_prs") as mock_prs,
        patch.object(_mod, "delete_remote_branches") as mock_branches,
        patch.object(_mod, "reset_issues") as mock_issues,
        patch.object(_mod, "clean_local") as mock_clean,
        patch.object(_mod, "load_issue_template") as mock_template,
    ):
        mock_git.return_value = {"status": "ok"}
        mock_prs.return_value = 0
        mock_branches.return_value = 0
        mock_template.return_value = []
        mock_issues.return_value = 0

        result = _mod.reset("owner/repo")

    assert result["status"] == "ok"
    mock_clean.assert_not_called()


def test_reset_git_failure_stops_early():
    """reset() returns error when reset_git fails."""
    with patch.object(_mod, "reset_git") as mock_git:
        mock_git.return_value = {"status": "error", "message": "not a repo"}

        result = _mod.reset("owner/repo", local_path="/tmp/repo")

    assert result["status"] == "error"


# --- CLI ---


def test_main_success():
    """main() prints JSON and exits 0 on success."""
    with patch.object(_mod, "reset") as mock_reset, patch("sys.argv", ["qa-reset", "--repo", "owner/repo"]):
        mock_reset.return_value = {
            "status": "ok",
            "prs_closed": 0,
            "branches_deleted": 0,
            "issues_reset": 0,
        }
        _mod.main()

    mock_reset.assert_called_once_with("owner/repo", local_path=None)


def test_main_with_local_path():
    """main() passes local_path when provided."""
    with (
        patch.object(_mod, "reset") as mock_reset,
        patch("sys.argv", ["qa-reset", "--repo", "owner/repo", "--local-path", "/tmp/repo"]),
    ):
        mock_reset.return_value = {
            "status": "ok",
            "prs_closed": 0,
            "branches_deleted": 0,
            "issues_reset": 0,
        }
        _mod.main()

    mock_reset.assert_called_once_with("owner/repo", local_path="/tmp/repo")


def test_main_error():
    """main() exits 1 on error."""
    with (
        patch.object(_mod, "reset") as mock_reset,
        patch("sys.argv", ["qa-reset", "--repo", "owner/repo"]),
        pytest.raises(SystemExit) as exc_info,
    ):
        mock_reset.return_value = {
            "status": "error",
            "message": "failed",
        }
        _mod.main()

    assert exc_info.value.code == 1


def test_cli_missing_args(monkeypatch):
    """Missing --repo exits with error."""
    monkeypatch.setattr("sys.argv", ["qa-reset"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code != 0
