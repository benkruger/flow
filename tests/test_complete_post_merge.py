"""Tests for lib/complete-post-merge.py — the consolidated Complete phase post-merge script."""

import json
import subprocess
import sys
from unittest.mock import patch

from conftest import LIB_DIR, import_lib, make_state, write_state

_mod = import_lib("complete-post-merge.py")

SCRIPT = str(LIB_DIR / "complete-post-merge.py")


# --- helpers ---


def _make_result(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=[],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


_PT_COMPLETE_OK = json.dumps(
    {
        "status": "ok",
        "phase": "flow-complete",
        "action": "complete",
        "cumulative_seconds": 45,
        "formatted_time": "<1m",
        "next_phase": "flow-complete",
        "continue_action": "invoke",
    }
)

_RENDER_PR_OK = '{"status": "ok", "sections": ["What"]}'

_ISSUES_SUMMARY_NO_ISSUES = '{"status": "ok", "has_issues": false, "banner_line": "", "table": ""}'

_ISSUES_SUMMARY_WITH_ISSUES = json.dumps(
    {
        "status": "ok",
        "has_issues": True,
        "banner_line": "Issues filed: 1 (Flaky Test: 1)",
        "table": "| Label | Title |",
    }
)

_CLOSE_ISSUES_EMPTY = '{"status": "ok", "closed": [], "failed": []}'

_CLOSE_ISSUES_WITH_CLOSED = json.dumps(
    {
        "status": "ok",
        "closed": [{"number": 100, "url": "https://github.com/test/test/issues/100"}],
        "failed": [],
    }
)

_SUMMARY_OK = json.dumps(
    {
        "status": "ok",
        "summary": "test summary",
        "total_seconds": 300,
        "issues_links": "",
    }
)

_LABEL_OK = '{"status": "ok", "labeled": [100], "failed": []}'

_AUTO_CLOSE_OK = '{"status": "ok", "parent_closed": false, "milestone_closed": false}'

_SLACK_SKIPPED = '{"status": "skipped", "reason": "no slack config"}'

_SLACK_OK = '{"status": "ok", "ts": "1234567890.123456"}'

_ADD_NOTIFICATION_OK = '{"status": "ok", "notification_count": 1}'


def _setup_post_merge_state(target_project, branch="test-feature", slack_thread_ts=None):
    """Create a state file ready for post-merge."""
    phase_statuses = {
        "flow-start": "complete",
        "flow-plan": "complete",
        "flow-code": "complete",
        "flow-code-review": "complete",
        "flow-learn": "complete",
    }
    state = make_state(current_phase="flow-complete", phase_statuses=phase_statuses)
    state["branch"] = branch
    state["pr_number"] = 42
    state["pr_url"] = "https://github.com/test/test/pull/42"
    state["repo"] = "test/test"
    state["prompt"] = "work on issue #100"
    state["complete_step"] = 5
    if slack_thread_ts:
        state["slack_thread_ts"] = slack_thread_ts
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    write_state(state_dir, branch, state)
    return state


# --- post_merge() in-process tests ---


def test_happy_path_no_issues(target_project, monkeypatch):
    """All operations succeed, no issues to close."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["status"] == "ok"
    assert result["formatted_time"] == "<1m"
    assert result["summary"] == "test summary"


def test_happy_path_with_closed_issues(target_project, monkeypatch):
    """Issues are closed, closed-issues file written, auto-close-parent called."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_WITH_CLOSED),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout=_AUTO_CLOSE_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["status"] == "ok"
    assert len(result["closed_issues"]) == 1
    # Closed issues file should be written
    closed_path = target_project / ".flow-states" / "test-feature-closed-issues.json"
    assert closed_path.exists()
    closed_data = json.loads(closed_path.read_text())
    assert closed_data[0]["number"] == 100


def test_individual_failure_continues(target_project, monkeypatch):
    """One subprocess fails, others still run (best-effort)."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        # label-issues fails
        _make_result(returncode=1, stderr="gh error"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["status"] == "ok"
    assert "label_issues" in result["failures"]


def test_slack_not_configured(target_project, monkeypatch):
    """No slack_thread_ts in state — slack is skipped."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["slack"]["status"] == "skipped"


def test_slack_succeeds(target_project, monkeypatch):
    """Slack thread_ts present, notify succeeds, add-notification called."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project, slack_thread_ts="1234.5678")

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout=_SLACK_OK),
        _make_result(stdout=_ADD_NOTIFICATION_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["slack"]["status"] == "ok"
    assert result["slack"]["ts"] == "1234567890.123456"


def test_phase_transition_called_with_next_phase(target_project, monkeypatch):
    """Phase transition is called with --next-phase flow-complete."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    calls_made = []
    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]
    response_iter = iter(responses)

    def _tracking_run(args, **kwargs):
        calls_made.append(args)
        return next(response_iter)

    with patch("subprocess.run", side_effect=_tracking_run):
        _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    pt_calls = [c for c in calls_made if "phase-transition" in str(c)]
    assert len(pt_calls) >= 1
    assert "--next-phase" in pt_calls[0]
    assert "flow-complete" in pt_calls[0]


def test_step_counters_updated(target_project, monkeypatch):
    """Post-merge updates complete_step in state file."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    state = json.loads((target_project / ".flow-states" / "test-feature.json").read_text())
    assert state["complete_step"] == 6


def test_phase_transition_failure(target_project, monkeypatch):
    """Phase transition failure is captured in failures dict."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(returncode=1, stdout='{"status": "error", "message": "bad state"}'),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    # Still ok overall — best effort
    assert result["status"] == "ok"
    assert "phase_transition" in result["failures"]


def test_slack_failure_continues(target_project, monkeypatch):
    """Slack failure is captured but doesn't block."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project, slack_thread_ts="1234.5678")

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout='{"status": "error", "message": "token expired"}'),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["slack"]["status"] == "error"


def test_corrupt_state_file(target_project, monkeypatch):
    """Corrupt state file is handled gracefully (no crash)."""
    monkeypatch.chdir(target_project)
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    (state_dir / "test-feature.json").write_text("not valid json{{{")

    responses = [
        _make_result(returncode=1, stdout='{"status": "error"}'),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["status"] == "ok"


def test_issues_summary_with_issues(target_project, monkeypatch):
    """Issues summary with has_issues=true populates banner_line."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_WITH_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["banner_line"] == "Issues filed: 1 (Flaky Test: 1)"


def test_closed_issues_file_write_error(target_project, monkeypatch):
    """OSError writing closed-issues file is captured in failures."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_WITH_CLOSED),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout=_AUTO_CLOSE_OK),
    ]

    # Make .flow-states read-only so write fails
    from pathlib import Path

    original_write = Path.write_text

    call_count = 0

    def _fail_write(self, *args, **kwargs):
        nonlocal call_count
        if "closed-issues" in str(self):
            call_count += 1
            if call_count == 1:
                raise OSError("permission denied")
        return original_write(self, *args, **kwargs)

    monkeypatch.setattr(Path, "write_text", _fail_write)

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert "closed_issues_file" in result["failures"]


def test_parent_closed(target_project, monkeypatch):
    """Auto-close-parent reporting parent_closed populates parents_closed."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    parent_closed_response = '{"status": "ok", "parent_closed": true, "milestone_closed": false}'
    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_WITH_CLOSED),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout=parent_closed_response),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert 100 in result["parents_closed"]


def test_slack_invalid_response(target_project, monkeypatch):
    """Invalid slack JSON response is handled."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project, slack_thread_ts="1234.5678")

    responses = [
        _make_result(stdout=_PT_COMPLETE_OK),
        _make_result(stdout=_RENDER_PR_OK),
        _make_result(stdout=_ISSUES_SUMMARY_NO_ISSUES),
        _make_result(stdout=_CLOSE_ISSUES_EMPTY),
        _make_result(stdout=_SUMMARY_OK),
        _make_result(stdout=_LABEL_OK),
        _make_result(stdout="not json"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    assert result["slack"]["status"] == "error"
    assert "invalid" in result["slack"]["message"].lower()


def test_timeout_handling(target_project, monkeypatch):
    """Subprocess timeout is handled gracefully."""
    monkeypatch.chdir(target_project)
    _setup_post_merge_state(target_project)

    def _timeout_run(args, **kwargs):
        raise subprocess.TimeoutExpired(cmd=args, timeout=30)

    with patch("subprocess.run", side_effect=_timeout_run):
        result = _mod.post_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            branch="test-feature",
            root=target_project,
        )

    # Still returns ok (best-effort) with failures recorded
    assert result["status"] == "ok"
    assert len(result["failures"]) > 0


# --- CLI tests ---


def test_cli_requires_args():
    """CLI without required args returns non-zero."""
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0


def test_cli_with_args(target_project):
    """CLI with required args runs without crashing on import."""
    _setup_post_merge_state(target_project)
    result = subprocess.run(
        [
            sys.executable,
            SCRIPT,
            "--pr",
            "42",
            "--state-file",
            str(target_project / ".flow-states" / "test-feature.json"),
            "--branch",
            "test-feature",
        ],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    assert result.returncode in (0, 1)
    data = json.loads(result.stdout.strip().splitlines()[-1])
    assert "status" in data
