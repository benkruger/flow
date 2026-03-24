"""Tests for lib/format-issues-summary.py — formats issues summary for Complete phase."""

import json
import sys

from conftest import LIB_DIR, import_lib, make_state

SCRIPT = str(LIB_DIR / "format-issues-summary.py")


def _make_issues(*labels):
    """Create a list of issue dicts with the given labels."""
    issues = []
    for i, label in enumerate(labels, 1):
        issues.append({
            "label": label,
            "title": f"Issue {i}",
            "url": f"https://github.com/test/test/issues/{i}",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        })
    return issues


# --- In-process tests ---


def test_empty_issues_returns_no_issues():
    """Empty issues_filed returns has_issues=False."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = []

    result = mod.format_issues_summary(state)

    assert result["has_issues"] is False
    assert result["banner_line"] == ""
    assert result["table"] == ""


def test_missing_issues_filed_returns_no_issues():
    """State without issues_filed key returns has_issues=False."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    del state["issues_filed"]

    result = mod.format_issues_summary(state)

    assert result["has_issues"] is False


def test_single_issue_formats_correctly():
    """Single issue produces correct banner line and table."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = _make_issues("Rule")

    result = mod.format_issues_summary(state)

    assert result["has_issues"] is True
    assert result["banner_line"] == "Issues filed: 1 (Rule: 1)"
    assert "| Label | Title | Phase | URL |" in result["table"]
    assert "| Rule | Issue 1 | Learn |" in result["table"]


def test_multiple_labels_grouped():
    """Multiple issues with different labels are grouped correctly."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = _make_issues("Rule", "Flaky Test", "Rule", "Tech Debt")

    result = mod.format_issues_summary(state)

    assert result["has_issues"] is True
    assert result["banner_line"] == "Issues filed: 4 (Rule: 2, Flaky Test: 1, Tech Debt: 1)"


def test_table_contains_all_issues():
    """Table contains a row for each issue."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = _make_issues("Rule", "Flow")

    result = mod.format_issues_summary(state)

    lines = result["table"].strip().split("\n")
    header_and_separator = 2
    assert len(lines) == header_and_separator + 2


def test_table_url_is_short_reference():
    """Table shows issue number as link, not full URL."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = [{
        "label": "Rule",
        "title": "Test rule",
        "url": "https://github.com/test/test/issues/42",
        "phase": "flow-learn",
        "phase_name": "Learn",
        "timestamp": "2026-01-01T00:00:00-08:00",
    }]

    result = mod.format_issues_summary(state)

    assert "#42" in result["table"]


def test_label_order_preserved():
    """Labels appear in the order they are first encountered."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = _make_issues("Flaky Test", "Rule", "Flaky Test")

    result = mod.format_issues_summary(state)

    assert result["banner_line"] == "Issues filed: 3 (Flaky Test: 2, Rule: 1)"


# --- CLI behavior (in-process) ---


def test_cli_happy_path(tmp_path, monkeypatch, capsys):
    """Full CLI round-trip: write state, run CLI, verify output."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = _make_issues("Rule", "Flow")
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))
    output_path = tmp_path / "issues.md"

    monkeypatch.setattr("sys.argv", ["format-issues-summary.py",
                                      "--state-file", str(state_path),
                                      "--output", str(output_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["has_issues"] is True
    assert "Issues filed: 2" in data["banner_line"]
    assert output_path.exists()
    table_on_disk = output_path.read_text()
    assert "| Label | Title | Phase | URL |" in table_on_disk


def test_cli_no_issues(tmp_path, monkeypatch, capsys):
    """CLI with no issues returns has_issues=False and skips file write."""
    mod = import_lib("format-issues-summary.py")
    state = make_state()
    state["issues_filed"] = []
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))
    output_path = tmp_path / "issues.md"

    monkeypatch.setattr("sys.argv", ["format-issues-summary.py",
                                      "--state-file", str(state_path),
                                      "--output", str(output_path)])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["has_issues"] is False
    assert not output_path.exists()


def test_cli_missing_state_file(tmp_path, monkeypatch, capsys):
    """CLI with nonexistent state file returns error."""
    mod = import_lib("format-issues-summary.py")

    monkeypatch.setattr("sys.argv", ["format-issues-summary.py",
                                      "--state-file", str(tmp_path / "missing.json"),
                                      "--output", str(tmp_path / "out.md")])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_cli_corrupt_state_file(tmp_path, monkeypatch, capsys):
    """CLI with corrupt JSON returns error."""
    mod = import_lib("format-issues-summary.py")
    bad_file = tmp_path / "state.json"
    bad_file.write_text("{bad json")

    monkeypatch.setattr("sys.argv", ["format-issues-summary.py",
                                      "--state-file", str(bad_file),
                                      "--output", str(tmp_path / "out.md")])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
