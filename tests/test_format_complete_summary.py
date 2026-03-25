"""Tests for lib/format-complete-summary.py — formats the Done banner for Complete phase."""

import json
import sys

import pytest

from conftest import import_lib, make_state, PHASE_NAMES


def _all_complete_state(**overrides):
    """Build a state dict with all phases complete and realistic timings."""
    statuses = {key: "complete" for key in PHASE_NAMES}
    state = make_state(current_phase="flow-complete", phase_statuses=statuses)
    # Set realistic cumulative_seconds for each phase
    timings = {
        "flow-start": 20,
        "flow-plan": 300,
        "flow-code": 2700,
        "flow-code-review": 720,
        "flow-learn": 120,
        "flow-complete": 45,
    }
    for key, seconds in timings.items():
        state["phases"][key]["cumulative_seconds"] = seconds
    state["prompt"] = "Add invoice PDF export with watermark support"
    for key, value in overrides.items():
        state[key] = value
    return state


# --- In-process tests ---


def test_basic_summary():
    """Summary contains feature name, prompt, PR URL, all phase names, and total."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()

    result = mod.format_complete_summary(state)

    summary = result["summary"]
    assert "Test Feature" in summary
    assert "Add invoice PDF export with watermark support" in summary
    assert "https://github.com/test/test/pull/1" in summary
    for name in PHASE_NAMES.values():
        assert f"{name}:" in summary
    assert "Total:" in summary
    assert result["total_seconds"] == 20 + 300 + 2700 + 720 + 120 + 45


def test_summary_with_issues():
    """Summary includes issues filed with #N shorthand from short_issue_ref."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["issues_filed"] = [
        {
            "label": "Rule",
            "title": "Test rule",
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/2",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]

    result = mod.format_complete_summary(state)

    assert "Issues filed: 2" in result["summary"]
    # #N shorthand appears in the label line
    assert "[Rule] #1 Test rule" in result["summary"]
    assert "[Tech Debt] #2 Refactor X" in result["summary"]
    # URLs still on next line
    assert "https://github.com/test/test/issues/1" in result["summary"]
    assert "https://github.com/test/test/issues/2" in result["summary"]


def test_summary_with_single_issue():
    """Summary lists a single issue with label, #N shorthand, and title."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["issues_filed"] = [
        {
            "label": "Flow",
            "title": "Fix routing logic",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]

    result = mod.format_complete_summary(state)

    assert "Issues filed: 1" in result["summary"]
    assert "[Flow] #42 Fix routing logic" in result["summary"]
    assert "https://github.com/test/test/issues/42" in result["summary"]


def test_summary_with_issues_url_without_number():
    """Issues with non-standard URLs fall back to full URL."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["issues_filed"] = [
        {
            "label": "Rule",
            "title": "Some rule",
            "url": "https://example.com/custom-path",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]

    result = mod.format_complete_summary(state)

    assert "Issues filed: 1" in result["summary"]
    assert "[Rule] Some rule" in result["summary"]
    assert "https://example.com/custom-path" in result["summary"]
    # Old colon-joined format must not appear
    assert "https://example.com/custom-path: Some rule" not in result["summary"]


def test_summary_with_resolved_issues():
    """Summary includes Resolved section when closed_issues provided."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    closed = [
        {"number": 407, "url": "https://github.com/test/test/issues/407"},
    ]

    result = mod.format_complete_summary(state, closed_issues=closed)

    assert "Resolved" in result["summary"]
    assert "#407" in result["summary"]
    assert "https://github.com/test/test/issues/407" in result["summary"]


def test_summary_with_multiple_resolved_issues():
    """Summary lists each resolved issue on its own line."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    closed = [
        {"number": 83, "url": "https://github.com/test/test/issues/83"},
        {"number": 89, "url": "https://github.com/test/test/issues/89"},
    ]

    result = mod.format_complete_summary(state, closed_issues=closed)

    assert "#83" in result["summary"]
    assert "#89" in result["summary"]


def test_summary_no_resolved_issues():
    """Summary omits Resolved section when closed_issues is empty or None."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()

    result_none = mod.format_complete_summary(state, closed_issues=None)
    result_empty = mod.format_complete_summary(state, closed_issues=[])

    assert "Resolved" not in result_none["summary"]
    assert "Resolved" not in result_empty["summary"]


def test_summary_with_resolved_and_filed():
    """Summary includes both Resolved and Issues filed sections."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/50",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]
    closed = [
        {"number": 407, "url": "https://github.com/test/test/issues/407"},
    ]

    result = mod.format_complete_summary(state, closed_issues=closed)

    assert "Resolved" in result["summary"]
    assert "#407" in result["summary"]
    assert "Issues filed: 1" in result["summary"]
    assert "[Tech Debt] #50 Refactor X" in result["summary"]


def test_summary_resolved_without_url():
    """Resolved issues without url show only #N."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    closed = [{"number": 42}]

    result = mod.format_complete_summary(state, closed_issues=closed)

    assert "Resolved" in result["summary"]
    assert "#42" in result["summary"]


def test_summary_with_notes():
    """Summary includes notes captured count when notes exist."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["notes"] = [
        {
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T00:00:00-08:00",
            "type": "correction",
            "note": "Test note",
        },
    ]

    result = mod.format_complete_summary(state)

    assert "Notes captured: 1" in result["summary"]


def test_summary_no_issues_no_notes():
    """Summary omits artifact lines when no issues and no notes."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state["issues_filed"] = []
    state["notes"] = []

    result = mod.format_complete_summary(state)

    assert "Issues filed" not in result["summary"]
    assert "Notes captured" not in result["summary"]


def test_summary_truncates_long_prompt():
    """Prompt over 80 chars is truncated with ellipsis."""
    mod = import_lib("format-complete-summary.py")
    long_prompt = "A" * 100
    state = _all_complete_state(prompt=long_prompt)

    result = mod.format_complete_summary(state)

    assert long_prompt not in result["summary"]
    assert "..." in result["summary"]
    # The truncated prompt should be 80 chars + ellipsis
    assert "A" * 80 + "..." in result["summary"]


def test_summary_short_prompt_not_truncated():
    """Prompt under 80 chars is not truncated."""
    mod = import_lib("format-complete-summary.py")
    short_prompt = "Fix login bug"
    state = _all_complete_state(prompt=short_prompt)

    result = mod.format_complete_summary(state)

    assert short_prompt in result["summary"]
    assert "..." not in result["summary"]


def test_summary_uses_format_time():
    """Phase timings use format_time conventions."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    # flow-start has 20s → "<1m"
    # flow-code has 2700s → "45m"
    # flow-plan has 300s → "5m"

    result = mod.format_complete_summary(state)

    assert "<1m" in result["summary"]
    assert "45m" in result["summary"]
    assert "5m" in result["summary"]


def test_read_version_fallback_on_error(tmp_path):
    """read_version_from returns '?' when plugin.json cannot be read."""
    from flow_utils import read_version_from
    assert read_version_from(tmp_path / "nonexistent.json") == "?"


def test_summary_heavy_borders():
    """Summary uses heavy horizontal borders (━━)."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()

    result = mod.format_complete_summary(state)

    assert "━━" in result["summary"]


def test_summary_check_mark():
    """Summary includes ✓ marker."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()

    result = mod.format_complete_summary(state)

    assert "✓" in result["summary"]


def test_summary_version():
    """Summary includes FLOW version."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()

    result = mod.format_complete_summary(state)

    assert "FLOW v" in result["summary"]


# --- CLI behavior (in-process main()) ---


def test_cli_happy_path(tmp_path, monkeypatch, capsys):
    """Full CLI round-trip: write state, run CLI, verify JSON output."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["format-complete-summary.py",
                                      "--state-file", str(state_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert "Test Feature" in data["summary"]
    assert isinstance(data["total_seconds"], int)


def test_cli_with_closed_issues_file(tmp_path, monkeypatch, capsys):
    """CLI with --closed-issues-file includes Resolved section."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    closed = [
        {"number": 407, "url": "https://github.com/test/test/issues/407"},
    ]
    closed_path = tmp_path / "closed.json"
    closed_path.write_text(json.dumps(closed))

    monkeypatch.setattr("sys.argv", ["format-complete-summary.py",
                                      "--state-file", str(state_path),
                                      "--closed-issues-file", str(closed_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert "Resolved" in data["summary"]
    assert "#407" in data["summary"]


def test_cli_missing_closed_issues_file(tmp_path, monkeypatch, capsys):
    """CLI with nonexistent --closed-issues-file gracefully omits Resolved section."""
    mod = import_lib("format-complete-summary.py")
    state = _all_complete_state()
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    monkeypatch.setattr("sys.argv", ["format-complete-summary.py",
                                      "--state-file", str(state_path),
                                      "--closed-issues-file",
                                      str(tmp_path / "nonexistent.json")])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert "Resolved" not in data["summary"]


def test_cli_missing_state_file(tmp_path, monkeypatch, capsys):
    """CLI with nonexistent state file returns error."""
    mod = import_lib("format-complete-summary.py")

    monkeypatch.setattr("sys.argv", ["format-complete-summary.py",
                                      "--state-file",
                                      str(tmp_path / "missing.json")])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_cli_corrupt_state_file(tmp_path, monkeypatch, capsys):
    """CLI with corrupt JSON returns error."""
    mod = import_lib("format-complete-summary.py")
    bad_file = tmp_path / "state.json"
    bad_file.write_text("{bad json")

    monkeypatch.setattr("sys.argv", ["format-complete-summary.py",
                                      "--state-file", str(bad_file)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
