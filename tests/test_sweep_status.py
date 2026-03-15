"""Tests for lib/sweep-status.py — formats the sweep dashboard."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "sweep-status.py")


def _import_module():
    """Import sweep-status.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "sweep_status", LIB_DIR / "sweep-status.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _make_sweep(issues=None, status="in_progress"):
    """Build a minimal sweep state dict."""
    return {
        "started_at": "2026-03-15T10:00:00-07:00",
        "status": status,
        "concurrency_limit": 3,
        "issues": issues or [],
    }


def _make_issue(number, title, status="queued", pr_number=None, pr_url=None):
    """Build a minimal issue entry."""
    return {
        "number": number,
        "title": title,
        "status": status,
        "branch": f"sweep/{number}" if status != "queued" else None,
        "worktree": f".worktrees/sweep-{number}" if status != "queued" else None,
        "pr_number": pr_number,
        "pr_url": pr_url,
        "agent_name": f"worker-{number}",
        "started_at": "2026-03-15T10:01:00-07:00" if status != "queued" else None,
        "completed_at": "2026-03-15T10:15:00-07:00" if status == "complete" else None,
        "error": None,
    }


# --- In-process tests ---


def test_read_version_returns_string():
    """_read_version returns a version string or '?' fallback."""
    mod = _import_module()
    result = mod._read_version()
    # Should return either the real version or the fallback
    assert isinstance(result, str)
    assert len(result) > 0


def test_read_version_fallback(tmp_path, monkeypatch):
    """_read_version returns '?' when plugin.json is missing."""
    mod = _import_module()
    # Temporarily make the script think it lives in a different directory
    import types
    original_file = mod.__file__
    mod.__file__ = str(tmp_path / "lib" / "sweep-status.py")
    result = mod._read_version()
    mod.__file__ = original_file
    assert result == "?"


def test_format_dashboard_no_issues():
    """Dashboard with empty issues list shows zero total."""
    mod = _import_module()
    sweep = _make_sweep(issues=[])
    result = mod.format_dashboard(sweep, "0.29.0")
    assert "Issues: 0 total" in result
    assert "FLOW v0.29.0" in result
    assert "Sweep Status" in result


def test_format_dashboard_mixed_statuses():
    """Dashboard with mixed statuses shows correct counts."""
    mod = _import_module()
    issues = [
        _make_issue(42, "Fix login timeout", status="complete", pr_number=156, pr_url="https://github.com/test/repo/pull/156"),
        _make_issue(43, "Add dark mode toggle", status="in_progress"),
        _make_issue(45, "Update email templates", status="in_progress"),
        _make_issue(47, "Refactor auth middleware", status="queued"),
        _make_issue(51, "Fix CSV export encoding", status="failed"),
    ]
    sweep = _make_sweep(issues=issues)
    result = mod.format_dashboard(sweep, "0.29.0")

    assert "Issues: 5 total" in result
    assert "1 complete" in result
    assert "2 in progress" in result
    assert "1 queued" in result
    assert "1 failed" in result
    assert "#156" in result
    assert "Fix login timeout" in result


def test_format_dashboard_all_complete():
    """Dashboard with all complete issues."""
    mod = _import_module()
    issues = [
        _make_issue(42, "Fix login", status="complete", pr_number=10, pr_url="url"),
        _make_issue(43, "Fix logout", status="complete", pr_number=11, pr_url="url"),
    ]
    sweep = _make_sweep(issues=issues, status="complete")
    result = mod.format_dashboard(sweep, "0.29.0")

    assert "Issues: 2 total" in result
    assert "2 complete" in result


def test_format_dashboard_pr_display():
    """Issues without PRs show dash, issues with PRs show number."""
    mod = _import_module()
    issues = [
        _make_issue(42, "Has PR", status="complete", pr_number=99, pr_url="url"),
        _make_issue(43, "No PR", status="queued"),
    ]
    sweep = _make_sweep(issues=issues)
    result = mod.format_dashboard(sweep, "0.29.0")

    lines = result.split("\n")
    issue_42_line = [line for line in lines if "Has PR" in line][0]
    issue_43_line = [line for line in lines if "No PR" in line][0]
    assert "#99" in issue_42_line
    assert "—" in issue_43_line


def test_format_dashboard_long_title_truncated():
    """Titles longer than 30 chars are truncated."""
    mod = _import_module()
    issues = [
        _make_issue(1, "A very long title that exceeds thirty characters easily"),
    ]
    sweep = _make_sweep(issues=issues)
    result = mod.format_dashboard(sweep, "0.29.0")

    lines = result.split("\n")
    issue_line = [line for line in lines if "1 |" in line or "  1" in line][0]
    # The full title should NOT appear
    assert "easily" not in issue_line


# --- CLI behavior (subprocess) ---


def test_cli_exit_1_when_no_sweep_file(git_repo):
    """Running with no sweep.json exits with code 1."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 1


def test_cli_exit_0_with_dashboard(git_repo):
    """Running with valid sweep.json prints dashboard and exits 0."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test issue", status="in_progress"),
    ])
    (state_dir / "sweep.json").write_text(json.dumps(sweep))

    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 0
    assert "Sweep Status" in result.stdout
    assert "Test issue" in result.stdout


def test_cli_exit_2_on_corrupt_file(git_repo):
    """Corrupt sweep.json exits with code 2."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "sweep.json").write_text("{bad json")

    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 2
    assert "Error" in result.stderr
