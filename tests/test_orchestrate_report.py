"""Tests for lib/orchestrate-report.py — generates morning report from orchestration state."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "orchestrate-report.py")


def _import_module():
    """Import orchestrate-report.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "orchestrate_report", LIB_DIR / "orchestrate-report.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _make_orchestrate_state(
    queue_items=None,
    started_at="2026-03-20T22:00:00-07:00",
    completed_at="2026-03-21T06:00:00-07:00",
):
    """Build a sample orchestrate state dict."""
    if queue_items is None:
        queue_items = []
    return {
        "started_at": started_at,
        "completed_at": completed_at,
        "queue": queue_items,
        "current_index": None,
    }


def _completed_item(issue_number, title, pr_url=None, branch=None):
    """Build a completed queue item."""
    return {
        "issue_number": issue_number,
        "title": title,
        "status": "completed",
        "started_at": "2026-03-20T22:05:00-07:00",
        "completed_at": "2026-03-20T23:00:00-07:00",
        "outcome": "completed",
        "pr_url": pr_url or f"https://github.com/test/test/pull/{issue_number}",
        "branch": branch or f"issue-{issue_number}",
        "reason": None,
    }


def _failed_item(issue_number, title, reason="CI failed after 3 attempts"):
    """Build a failed queue item."""
    return {
        "issue_number": issue_number,
        "title": title,
        "status": "failed",
        "started_at": "2026-03-20T22:05:00-07:00",
        "completed_at": "2026-03-20T22:30:00-07:00",
        "outcome": "failed",
        "pr_url": None,
        "branch": None,
        "reason": reason,
    }


# --- In-process tests ---


def test_report_all_completed():
    """Report shows all issues as completed."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _completed_item(43, "Fix login timeout"),
    ])

    result = mod.generate_report(state)

    assert result["completed"] == 2
    assert result["failed"] == 0
    assert result["total"] == 2
    assert "#42" in result["summary"]
    assert "#43" in result["summary"]
    assert "completed" in result["summary"].lower()


def test_report_mixed_results():
    """Report shows both completed and failed issues."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login timeout"),
    ])

    result = mod.generate_report(state)

    assert result["completed"] == 1
    assert result["failed"] == 1
    assert result["total"] == 2
    assert "#42" in result["summary"]
    assert "#43" in result["summary"]


def test_report_all_failed():
    """Report shows all issues as failed."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _failed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login timeout"),
    ])

    result = mod.generate_report(state)

    assert result["completed"] == 0
    assert result["failed"] == 2
    assert result["total"] == 2


def test_report_empty_queue():
    """Report handles empty queue gracefully."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[])

    result = mod.generate_report(state)

    assert result["completed"] == 0
    assert result["failed"] == 0
    assert result["total"] == 0


def test_report_single_issue():
    """Report works with a single completed issue."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
    ])

    result = mod.generate_report(state)

    assert result["completed"] == 1
    assert result["total"] == 1
    assert "#42" in result["summary"]
    assert "Add PDF export" in result["summary"]


def test_report_includes_timing():
    """Report includes duration based on started_at and completed_at."""
    mod = _import_module()
    state = _make_orchestrate_state(
        queue_items=[_completed_item(42, "Add PDF export")],
        started_at="2026-03-20T22:00:00-07:00",
        completed_at="2026-03-21T06:00:00-07:00",
    )

    result = mod.generate_report(state)

    assert "8h" in result["summary"]


def test_report_includes_pr_urls():
    """Report includes PR URLs for completed issues."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export",
                        pr_url="https://github.com/test/test/pull/100"),
    ])

    result = mod.generate_report(state)

    assert "https://github.com/test/test/pull/100" in result["summary"]


def test_report_includes_failure_reasons():
    """Report includes failure reasons for failed issues."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _failed_item(43, "Fix login timeout", reason="CI failed after 3 attempts"),
    ])

    result = mod.generate_report(state)

    assert "CI failed after 3 attempts" in result["summary"]


def test_report_writes_summary_file(tmp_path):
    """Report writes summary to .flow-states/orchestrate-summary.md."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
    ])
    state_path = tmp_path / "orchestrate.json"
    state_path.write_text(json.dumps(state))

    result = mod.generate_and_write_report(str(state_path), str(tmp_path))

    assert result["status"] == "ok"
    summary_path = tmp_path / "orchestrate-summary.md"
    assert summary_path.exists()
    content = summary_path.read_text()
    assert "#42" in content
    assert "Add PDF export" in content


def test_report_bad_timestamps():
    """Report handles invalid timestamps gracefully (duration shows <1m)."""
    mod = _import_module()
    state = _make_orchestrate_state(
        queue_items=[_completed_item(42, "Add PDF export")],
        started_at="not-a-timestamp",
        completed_at="also-not-a-timestamp",
    )

    result = mod.generate_report(state)

    assert "<1m" in result["summary"]


def test_report_results_table_format():
    """Report results table has the expected column headers."""
    mod = _import_module()
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login"),
    ])

    result = mod.generate_report(state)

    assert "| #" in result["summary"]
    assert "Issue" in result["summary"]
    assert "Outcome" in result["summary"]


# --- CLI integration tests ---


def test_cli_happy_path(tmp_path):
    """CLI generates report from state file."""
    state = _make_orchestrate_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login"),
    ])
    state_path = tmp_path / "orchestrate.json"
    state_path.write_text(json.dumps(state))

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--state-file", str(state_path),
         "--output-dir", str(tmp_path)],
        capture_output=True, text=True,
    )

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["completed"] == 1
    assert data["failed"] == 1
    assert (tmp_path / "orchestrate-summary.md").exists()


def test_cli_missing_state_file(tmp_path):
    """CLI with nonexistent state file returns error."""
    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--state-file", str(tmp_path / "missing.json"),
         "--output-dir", str(tmp_path)],
        capture_output=True, text=True,
    )

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_cli_corrupt_state_file(tmp_path):
    """CLI with corrupt JSON returns error."""
    bad_file = tmp_path / "orchestrate.json"
    bad_file.write_text("{bad json")

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--state-file", str(bad_file),
         "--output-dir", str(tmp_path)],
        capture_output=True, text=True,
    )

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
