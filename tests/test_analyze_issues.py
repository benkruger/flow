"""Tests for lib/analyze-issues.py — mechanical analysis of open GitHub issues."""

import importlib
import json
import subprocess
import sys
from datetime import datetime, timedelta
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("analyze-issues")


# --- helpers ---

def _make_issue(number, title="Test issue", body="", labels=None,
                created_at=None, url=None):
    """Build a minimal gh issue list JSON entry."""
    if created_at is None:
        created_at = datetime.now().isoformat()
    if url is None:
        url = f"https://github.com/test/repo/issues/{number}"
    return {
        "number": number,
        "title": title,
        "body": body or "",
        "labels": [{"name": n} for n in (labels or [])],
        "createdAt": created_at,
        "url": url,
    }


# --- extract_file_paths ---


def test_extracts_directory_prefixed_paths():
    """Recognizes paths with known directory prefixes."""
    body = "Check lib/foo.py and skills/bar/SKILL.md for details."
    result = _mod.extract_file_paths(body)
    assert "lib/foo.py" in result
    assert "skills/bar/SKILL.md" in result


def test_extracts_paths_with_file_extensions():
    """Recognizes paths with file extensions even without known prefixes."""
    body = "See config/setup.json and src/main.sh"
    result = _mod.extract_file_paths(body)
    assert "config/setup.json" in result
    assert "src/main.sh" in result


def test_no_file_paths():
    """Returns empty list when body has no file paths."""
    result = _mod.extract_file_paths("This is a plain description.")
    assert result == []


def test_deduplicates_file_paths():
    """Duplicate file paths are returned only once."""
    body = "Check lib/foo.py and also lib/foo.py again"
    result = _mod.extract_file_paths(body)
    assert result.count("lib/foo.py") == 1


def test_extracts_dotprefix_paths():
    """Recognizes paths starting with dot directories like .claude/."""
    body = "Edit .claude/rules/testing.md"
    result = _mod.extract_file_paths(body)
    assert ".claude/rules/testing.md" in result


# --- extract_dependencies ---


def test_extracts_dependencies_within_open_set():
    """Only records dependencies between issues in the open set."""
    body = "Depends on #10 and #20"
    open_numbers = {10, 20, 30}
    result = _mod.extract_dependencies(body, open_numbers)
    assert result == [10, 20]


def test_ignores_dependencies_outside_open_set():
    """References to closed or non-existent issues are ignored."""
    body = "Depends on #10 and #99"
    open_numbers = {10, 20}
    result = _mod.extract_dependencies(body, open_numbers)
    assert result == [10]


def test_no_dependencies():
    """Returns empty list when body has no #N patterns."""
    result = _mod.extract_dependencies("Plain text", {10, 20})
    assert result == []


def test_self_reference_excluded():
    """An issue referencing its own number is not a dependency."""
    body = "This is #5, depends on #10"
    open_numbers = {5, 10}
    result = _mod.extract_dependencies(body, open_numbers, own_number=5)
    assert result == [10]


# --- detect_labels ---


def test_detects_in_progress_label():
    """Issues with Flow In-Progress label are flagged."""
    labels = [{"name": "Flow In-Progress"}, {"name": "Bug"}]
    result = _mod.detect_labels(labels)
    assert result["in_progress"] is True
    assert result["decomposed"] is False


def test_detects_decomposed_label():
    """Issues with Decomposed label are flagged."""
    labels = [{"name": "Decomposed"}]
    result = _mod.detect_labels(labels)
    assert result["decomposed"] is True
    assert result["in_progress"] is False


def test_no_special_labels():
    """Issues without special labels have both flags False."""
    labels = [{"name": "Bug"}]
    result = _mod.detect_labels(labels)
    assert result["in_progress"] is False
    assert result["decomposed"] is False


def test_empty_labels():
    """Empty label list has both flags False."""
    result = _mod.detect_labels([])
    assert result["in_progress"] is False
    assert result["decomposed"] is False


# --- categorize ---


def test_categorize_by_label():
    """Label-based categories take precedence."""
    assert _mod.categorize({"Flaky Test"}, "Some title", "body") == "Flaky Test"


def test_categorize_rule_label():
    """Rule label maps to Rule category."""
    assert _mod.categorize({"Rule"}, "title", "body") == "Rule"


def test_categorize_flow_label():
    """Flow label maps to Flow category."""
    assert _mod.categorize({"Flow"}, "title", "body") == "Flow"


def test_categorize_tech_debt_label():
    """Tech Debt label maps to Tech Debt category."""
    assert _mod.categorize({"Tech Debt"}, "title", "body") == "Tech Debt"


def test_categorize_documentation_drift_label():
    """Documentation Drift label maps to Documentation Drift category."""
    assert _mod.categorize({"Documentation Drift"}, "title", "body") == "Documentation Drift"


def test_categorize_bug_by_content():
    """Content fallback detects bug keywords."""
    assert _mod.categorize(set(), "Fix crash on login", "error when") == "Bug"


def test_categorize_enhancement_by_content():
    """Content fallback detects enhancement keywords."""
    assert _mod.categorize(set(), "Add dark mode", "new feature") == "Enhancement"


def test_categorize_other_fallback():
    """Falls back to Other when no match."""
    assert _mod.categorize(set(), "Misc cleanup", "tidy up") == "Other"


# --- stale detection ---


def test_stale_issue_with_missing_files():
    """Issue >60 days old with missing file refs is marked stale."""
    with patch("os.path.exists", return_value=False):
        result = _mod.check_stale(["lib/missing.py"], 90)
    assert result["stale"] is True
    assert result["stale_missing"] == 1


def test_not_stale_when_files_exist():
    """Issue >60 days old with all files present is not stale."""
    with patch("os.path.exists", return_value=True):
        result = _mod.check_stale(["lib/exists.py"], 90)
    assert result["stale"] is False
    assert result["stale_missing"] == 0


def test_not_stale_when_recent():
    """Issue <60 days old is never stale regardless of files."""
    result = _mod.check_stale(["lib/missing.py"], 10)
    assert result["stale"] is False


def test_not_stale_when_no_file_paths():
    """Issue >60 days old but without file refs is not stale."""
    result = _mod.check_stale([], 90)
    assert result["stale"] is False


# --- build_dependents ---


def test_build_dependents():
    """Builds reverse dependency map from dependencies."""
    deps = {1: [2, 3], 4: [2]}
    result = _mod.build_dependents(deps)
    assert sorted(result[2]) == [1, 4]
    assert result[3] == [1]


def test_build_dependents_empty():
    """Empty dependency map returns empty dependents."""
    assert _mod.build_dependents({}) == {}


# --- truncate_body ---


def test_truncate_body_short():
    """Short body is returned as-is."""
    assert _mod.truncate_body("short text", 200) == "short text"


def test_truncate_body_long():
    """Long body is truncated with ellipsis."""
    body = "x" * 300
    result = _mod.truncate_body(body, 200)
    assert len(result) <= 203  # 200 + "..."
    assert result.endswith("...")


# --- analyze_issues (integration) ---


def test_analyze_empty_list():
    """Empty issue list returns empty result."""
    result = _mod.analyze_issues([])
    assert result["status"] == "ok"
    assert result["total"] == 0
    assert result["in_progress"] == []
    assert result["issues"] == []


def test_analyze_separates_in_progress():
    """In-progress issues go to in_progress array, not issues."""
    issues = [
        _make_issue(1, title="Active", labels=["Flow In-Progress"]),
        _make_issue(2, title="Available"),
    ]
    result = _mod.analyze_issues(issues)
    assert len(result["in_progress"]) == 1
    assert result["in_progress"][0]["number"] == 1
    assert len(result["issues"]) == 1
    assert result["issues"][0]["number"] == 2


def test_analyze_issue_fields():
    """Each analyzed issue has all expected fields."""
    issues = [_make_issue(1, title="Test", body="Check lib/foo.py",
                          labels=["Decomposed"])]
    result = _mod.analyze_issues(issues)
    issue = result["issues"][0]
    assert issue["number"] == 1
    assert issue["title"] == "Test"
    assert "url" in issue
    assert issue["labels"] == ["Decomposed"]
    assert issue["decomposed"] is True
    assert "age_days" in issue
    assert "file_paths" in issue
    assert "dependencies" in issue
    assert "dependents" in issue
    assert "brief" in issue
    assert "category" in issue
    assert "stale" in issue
    assert "stale_missing" in issue


def test_analyze_dependency_graph():
    """Dependencies and dependents are correctly computed."""
    issues = [
        _make_issue(1, title="Base"),
        _make_issue(2, title="Depends on 1", body="Requires #1"),
    ]
    result = _mod.analyze_issues(issues)
    issue_2 = next(i for i in result["issues"] if i["number"] == 2)
    issue_1 = next(i for i in result["issues"] if i["number"] == 1)
    assert 1 in issue_2["dependencies"]
    assert 2 in issue_1["dependents"]


def test_analyze_stale_detection():
    """Stale issues are flagged with missing file count."""
    old_date = (datetime.now() - timedelta(days=90)).isoformat()
    issues = [_make_issue(1, body="Check lib/gone.py", created_at=old_date)]
    with patch("os.path.exists", return_value=False):
        result = _mod.analyze_issues(issues)
    issue = result["issues"][0]
    assert issue["stale"] is True
    assert issue["stale_missing"] == 1


def test_analyze_total_includes_all():
    """Total count includes both in-progress and available issues."""
    issues = [
        _make_issue(1, labels=["Flow In-Progress"]),
        _make_issue(2),
        _make_issue(3),
    ]
    result = _mod.analyze_issues(issues)
    assert result["total"] == 3


# --- CLI integration ---


def test_cli_with_issues_json_file(tmp_path, monkeypatch, capsys):
    """CLI reads issues from --issues-json file and outputs analysis."""
    issues = [_make_issue(1, title="Test issue", body="Check lib/foo.py")]
    json_file = tmp_path / "issues.json"
    json_file.write_text(json.dumps(issues))

    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file)])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "ok"
    assert output["total"] == 1


def test_cli_empty_json_file(tmp_path, monkeypatch, capsys):
    """CLI handles empty issue list gracefully."""
    json_file = tmp_path / "issues.json"
    json_file.write_text("[]")

    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file)])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["total"] == 0


def test_cli_malformed_json(tmp_path, monkeypatch, capsys):
    """CLI returns error on malformed JSON input."""
    json_file = tmp_path / "issues.json"
    json_file.write_text("{corrupt")

    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file)])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code == 1
    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


def test_cli_missing_file(monkeypatch, capsys):
    """CLI returns error when --issues-json file does not exist."""
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", "/nonexistent/file.json"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code == 1
    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


# --- main() gh subprocess path ---


def test_main_calls_gh_when_no_file():
    """main() calls gh issue list when --issues-json is not provided."""
    gh_output = json.dumps([_make_issue(1, title="From GH")])
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=gh_output, stderr="",
        )
        with patch("sys.argv", ["analyze-issues"]):
            with patch("builtins.print") as mock_print:
                _mod.main()

    mock_run.assert_called_once()
    call_args = mock_run.call_args[0][0]
    assert call_args[0] == "gh"
    assert "issue" in call_args
    printed = mock_print.call_args[0][0]
    output = json.loads(printed)
    assert output["status"] == "ok"
    assert output["total"] == 1


def test_main_gh_failure():
    """main() exits with error when gh returns non-zero."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="auth required",
        )
        with patch("sys.argv", ["analyze-issues"]):
            with patch("builtins.print") as mock_print:
                with pytest.raises(SystemExit) as exc_info:
                    _mod.main()

    assert exc_info.value.code == 1
    printed = mock_print.call_args[0][0]
    output = json.loads(printed)
    assert output["status"] == "error"
    assert "failed" in output["message"]


def test_main_gh_timeout():
    """main() exits with error when gh times out."""
    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="gh", timeout=30)):
        with patch("sys.argv", ["analyze-issues"]):
            with patch("builtins.print") as mock_print:
                with pytest.raises(SystemExit) as exc_info:
                    _mod.main()

    assert exc_info.value.code == 1
    printed = mock_print.call_args[0][0]
    output = json.loads(printed)
    assert output["status"] == "error"
    assert "timed out" in output["message"]


# --- filter_issues ---


def test_filter_ready_returns_no_dependencies():
    """Ready filter returns only issues with empty dependencies array."""
    issues = [
        {"number": 1, "dependencies": [], "decomposed": False},
        {"number": 2, "dependencies": [1], "decomposed": False},
        {"number": 3, "dependencies": [], "decomposed": True},
    ]
    result = _mod.filter_issues(issues, "ready")
    assert [i["number"] for i in result] == [1, 3]


def test_filter_blocked_returns_has_dependencies():
    """Blocked filter returns only issues with non-empty dependencies array."""
    issues = [
        {"number": 1, "dependencies": [], "decomposed": False},
        {"number": 2, "dependencies": [1], "decomposed": False},
        {"number": 3, "dependencies": [1, 2], "decomposed": True},
    ]
    result = _mod.filter_issues(issues, "blocked")
    assert [i["number"] for i in result] == [2, 3]


def test_filter_decomposed_returns_decomposed_true():
    """Decomposed filter returns only issues with decomposed=True."""
    issues = [
        {"number": 1, "dependencies": [], "decomposed": False},
        {"number": 2, "dependencies": [1], "decomposed": True},
        {"number": 3, "dependencies": [], "decomposed": True},
    ]
    result = _mod.filter_issues(issues, "decomposed")
    assert [i["number"] for i in result] == [2, 3]


def test_filter_quick_start_returns_decomposed_and_ready():
    """Quick-start filter returns decomposed issues with no dependencies."""
    issues = [
        {"number": 1, "dependencies": [], "decomposed": False},
        {"number": 2, "dependencies": [1], "decomposed": True},
        {"number": 3, "dependencies": [], "decomposed": True},
    ]
    result = _mod.filter_issues(issues, "quick-start")
    assert [i["number"] for i in result] == [3]


def test_filter_none_returns_all():
    """No filter returns all issues unchanged."""
    issues = [
        {"number": 1, "dependencies": [], "decomposed": False},
        {"number": 2, "dependencies": [1], "decomposed": True},
    ]
    result = _mod.filter_issues(issues, None)
    assert result == issues


def test_filter_unknown_raises():
    """Invalid filter name raises ValueError."""
    with pytest.raises(ValueError, match="Unknown filter"):
        _mod.filter_issues([], "invalid")


# --- CLI filter flags ---


def _make_filter_issues_file(tmp_path):
    """Create a JSON file with mixed issues for filter testing."""
    issues = [
        _make_issue(1, title="Ready plain", body=""),
        _make_issue(2, title="Blocked", body="Depends on #1"),
        _make_issue(3, title="Decomposed ready", body="",
                    labels=["Decomposed"]),
        _make_issue(4, title="Decomposed blocked", body="Depends on #1",
                    labels=["Decomposed"]),
    ]
    json_file = tmp_path / "issues.json"
    json_file.write_text(json.dumps(issues))
    return json_file


def test_cli_ready_flag(tmp_path, monkeypatch, capsys):
    """--ready flag filters to issues with no dependencies."""
    json_file = _make_filter_issues_file(tmp_path)
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file), "--ready"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    numbers = [i["number"] for i in output["issues"]]
    assert 1 in numbers
    assert 3 in numbers
    assert 2 not in numbers
    assert 4 not in numbers
    assert output["total"] == 2


def test_cli_blocked_flag(tmp_path, monkeypatch, capsys):
    """--blocked flag filters to issues with dependencies."""
    json_file = _make_filter_issues_file(tmp_path)
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file), "--blocked"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    numbers = [i["number"] for i in output["issues"]]
    assert 2 in numbers
    assert 4 in numbers
    assert 1 not in numbers
    assert 3 not in numbers
    assert output["total"] == 2


def test_cli_decomposed_flag(tmp_path, monkeypatch, capsys):
    """--decomposed flag filters to decomposed issues."""
    json_file = _make_filter_issues_file(tmp_path)
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file), "--decomposed"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    numbers = [i["number"] for i in output["issues"]]
    assert 3 in numbers
    assert 4 in numbers
    assert 1 not in numbers
    assert 2 not in numbers
    assert output["total"] == 2


def test_cli_quick_start_flag(tmp_path, monkeypatch, capsys):
    """--quick-start flag filters to decomposed issues with no dependencies."""
    json_file = _make_filter_issues_file(tmp_path)
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file), "--quick-start"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    numbers = [i["number"] for i in output["issues"]]
    assert numbers == [3]
    assert output["total"] == 1


def test_cli_mutually_exclusive_flags(tmp_path, monkeypatch, capsys):
    """Passing two filter flags produces an error."""
    json_file = _make_filter_issues_file(tmp_path)
    monkeypatch.setattr("sys.argv", ["analyze-issues", "--issues-json", str(json_file), "--ready", "--blocked"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code != 0
