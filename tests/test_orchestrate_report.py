"""Tests for lib/orchestrate-report.py — generates morning report from orchestration state."""

import importlib.util
import json
import sys

from conftest import LIB_DIR, make_orchestrate_state


def _import_module():
    """Import orchestrate-report.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "orchestrate_report", LIB_DIR / "orchestrate-report.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _make_report_state(queue_items=None, **kwargs):
    """Build an orchestrate state for report tests (defaults completed_at)."""
    kwargs.setdefault("completed_at", "2026-03-21T06:00:00-07:00")
    return make_orchestrate_state(queue=queue_items, **kwargs)


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
    state = _make_report_state(queue_items=[
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
    state = _make_report_state(queue_items=[
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
    state = _make_report_state(queue_items=[
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
    state = _make_report_state(queue_items=[])

    result = mod.generate_report(state)

    assert result["completed"] == 0
    assert result["failed"] == 0
    assert result["total"] == 0


def test_report_single_issue():
    """Report works with a single completed issue."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
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
    state = _make_report_state(
        queue_items=[_completed_item(42, "Add PDF export")],
        started_at="2026-03-20T22:00:00-07:00",
        completed_at="2026-03-21T06:00:00-07:00",
    )

    result = mod.generate_report(state)

    assert "8h" in result["summary"]


def test_report_includes_pr_urls():
    """Report includes PR URLs for completed issues."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
        _completed_item(42, "Add PDF export",
                        pr_url="https://github.com/test/test/pull/100"),
    ])

    result = mod.generate_report(state)

    assert "https://github.com/test/test/pull/100" in result["summary"]


def test_report_includes_failure_reasons():
    """Report includes failure reasons for failed issues."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
        _failed_item(43, "Fix login timeout", reason="CI failed after 3 attempts"),
    ])

    result = mod.generate_report(state)

    assert "CI failed after 3 attempts" in result["summary"]


def test_report_writes_summary_file(tmp_path):
    """Report writes summary to .flow-states/orchestrate-summary.md."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
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


def test_compute_duration_none_completed_at():
    """_compute_duration_seconds returns 0 when completed_at is None.

    Documents the failure mode when orchestrate-report runs before
    orchestrate-state --complete (Bug 1 from #323).
    """
    mod = _import_module()

    assert mod._compute_duration_seconds("2026-03-20T22:00:00-07:00", None) == 0


def test_report_none_completed_at():
    """Report with None completed_at shows <1m duration (pre-fix behavior).

    When the report generates before --complete sets completed_at, the
    duration field falls back to <1m. This test documents the observable
    bug behavior from #323.
    """
    mod = _import_module()
    state = _make_report_state(
        queue_items=[_completed_item(42, "Add PDF export")],
        started_at="2026-03-20T22:00:00-07:00",
        completed_at=None,
    )

    result = mod.generate_report(state)

    assert "<1m" in result["summary"]


def test_report_bad_timestamps():
    """Report handles invalid timestamps gracefully (duration shows <1m)."""
    mod = _import_module()
    state = _make_report_state(
        queue_items=[_completed_item(42, "Add PDF export")],
        started_at="not-a-timestamp",
        completed_at="also-not-a-timestamp",
    )

    result = mod.generate_report(state)

    assert "<1m" in result["summary"]


def test_report_results_table_format():
    """Report results table has the expected column headers."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login"),
    ])

    result = mod.generate_report(state)

    assert "| #" in result["summary"]
    assert "Issue" in result["summary"]
    assert "Outcome" in result["summary"]


# --- CLI integration tests (in-process) ---


def test_cli_happy_path(tmp_path, monkeypatch, capsys):
    """CLI generates report from state file."""
    mod = _import_module()
    state = _make_report_state(queue_items=[
        _completed_item(42, "Add PDF export"),
        _failed_item(43, "Fix login"),
    ])
    state_path = tmp_path / "orchestrate.json"
    state_path.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["orchestrate-report.py",
                                      "--state-file", str(state_path),
                                      "--output-dir", str(tmp_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["completed"] == 1
    assert data["failed"] == 1
    assert (tmp_path / "orchestrate-summary.md").exists()


def test_cli_missing_state_file(tmp_path, monkeypatch, capsys):
    """CLI with nonexistent state file returns error."""
    mod = _import_module()

    monkeypatch.setattr("sys.argv", ["orchestrate-report.py",
                                      "--state-file", str(tmp_path / "missing.json"),
                                      "--output-dir", str(tmp_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_cli_corrupt_state_file(tmp_path, monkeypatch, capsys):
    """CLI with corrupt JSON returns error."""
    mod = _import_module()
    bad_file = tmp_path / "orchestrate.json"
    bad_file.write_text("{bad json")

    monkeypatch.setattr("sys.argv", ["orchestrate-report.py",
                                      "--state-file", str(bad_file),
                                      "--output-dir", str(tmp_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
