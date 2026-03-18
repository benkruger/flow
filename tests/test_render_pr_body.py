"""Tests for lib/render-pr-body.py — idempotent PR body rendering from state file."""

import importlib.util
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from conftest import LIB_DIR, make_state

SCRIPT = str(LIB_DIR / "render-pr-body.py")

# Import render-pr-body.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "render_pr_body", LIB_DIR / "render-pr-body.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- render_body (pure function, no subprocess) ---


def test_minimal_state(tmp_path):
    """Minimal state: What, Artifacts (empty), Phase Timings, State File."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 10
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert body.startswith("## What")
    assert "## Artifacts" in body
    assert "## Phase Timings" in body
    assert "## State File" in body
    # No plan, DAG, session log, or issues
    assert "## Plan" not in body
    assert "## DAG Analysis" not in body
    assert "## Session Log" not in body
    assert "## Issues Filed" not in body


def test_what_uses_prompt_over_feature(tmp_path):
    """What section uses prompt field when present, not the title-cased feature."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["prompt"] = "fix login timeout when session expires"
    state["feature"] = "Fix Login Timeout"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "fix login timeout when session expires." in body
    assert "Fix Login Timeout." not in body


def test_what_raises_on_empty_prompt(tmp_path):
    """Missing prompt is a bug — render_body raises ValueError."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["prompt"] = ""
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    with pytest.raises(ValueError, match="missing 'prompt'"):
        _mod.render_body(state, tmp_path)


def test_what_raises_when_no_prompt_key(tmp_path):
    """Missing prompt key is a bug — render_body raises ValueError."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    del state["prompt"]
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    with pytest.raises(ValueError, match="missing 'prompt'"):
        _mod.render_body(state, tmp_path)


def test_with_plan_only(tmp_path):
    """Plan file set and exists — Plan section appears."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    plan_file = tmp_path / "plan.md"
    plan_file.write_text("# My Plan\n\nDo the thing.")
    state["plan_file"] = str(plan_file)
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## Plan" in body
    assert "Do the thing." in body
    assert "## DAG Analysis" not in body


def test_with_plan_and_dag(tmp_path):
    """Both plan and DAG files set and exist — both sections appear."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    plan_file = tmp_path / "plan.md"
    plan_file.write_text("# Plan content")
    dag_file = tmp_path / "dag.md"
    dag_file.write_text("# DAG content")
    state["plan_file"] = str(plan_file)
    state["dag_file"] = str(dag_file)
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## Plan" in body
    assert "## DAG Analysis" in body
    assert "Plan content" in body
    assert "DAG content" in body


def test_dag_always_text_format(tmp_path):
    """DAG content with XML tags is wrapped in ```text, not ```xml."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    dag_file = tmp_path / "dag.md"
    dag_file.write_text('<dag goal="test"><node id="1"/></dag>')
    state["dag_file"] = str(dag_file)
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "```text" in body
    assert "```xml" not in body
    assert '<dag goal="test">' in body


def test_with_transcript(tmp_path):
    """Transcript path set — transcript appears in Artifacts table."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete"},
    )
    state["transcript_path"] = "/path/to/session.jsonl"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "| Transcript |" in body
    assert "/path/to/session.jsonl" in body


def test_full_state(tmp_path):
    """All phases complete, all files exist — all sections present."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "complete",
            "flow-complete": "complete",
        },
    )
    for key in state["phases"]:
        state["phases"][key]["started_at"] = "2026-01-01T00:00:00Z"
        state["phases"][key]["cumulative_seconds"] = 60

    plan_file = tmp_path / "plan.md"
    plan_file.write_text("Plan content")
    dag_file = tmp_path / "dag.md"
    dag_file.write_text("DAG content")
    log_file = tmp_path / ".flow-states" / "test-feature.log"
    log_file.parent.mkdir(parents=True, exist_ok=True)
    log_file.write_text("2026-01-01 [Phase 1] Step 1 — done")
    state["plan_file"] = str(plan_file)
    state["dag_file"] = str(dag_file)
    state["transcript_path"] = "/path/to/session.jsonl"
    state["issues_filed"] = [
        {"label": "Flow", "title": "Test issue", "url": "https://github.com/test/test/issues/1",
         "phase_name": "Learn"},
    ]

    body = _mod.render_body(state, tmp_path)

    assert "## What" in body
    assert "## Artifacts" in body
    assert "## Plan" in body
    assert "## DAG Analysis" in body
    assert "## Phase Timings" in body
    assert "## State File" in body
    assert "## Session Log" in body
    assert "## Issues Filed" in body


def test_with_issues(tmp_path):
    """Issues filed — Issues Filed section present."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={"flow-start": "complete"},
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["issues_filed"] = [
        {"label": "Rule", "title": "Add rule X", "url": "https://github.com/test/test/issues/5",
         "phase_name": "Learn"},
    ]

    body = _mod.render_body(state, tmp_path)

    assert "## Issues Filed" in body
    assert "Add rule X" in body


def test_plan_from_files_block(tmp_path):
    """Plan path in files block (relative) — Plan section appears."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    plan_dir = tmp_path / ".flow-states"
    plan_dir.mkdir(parents=True, exist_ok=True)
    plan_file = plan_dir / "test-feature-plan.md"
    plan_file.write_text("# Plan from files block")
    state["files"]["plan"] = ".flow-states/test-feature-plan.md"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## Plan" in body
    assert "Plan from files block" in body


def test_dag_from_files_block(tmp_path):
    """DAG path in files block (relative) — DAG Analysis section appears."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    dag_dir = tmp_path / ".flow-states"
    dag_dir.mkdir(parents=True, exist_ok=True)
    dag_file = dag_dir / "test-feature-dag.md"
    dag_file.write_text("# DAG from files block")
    state["files"]["dag"] = ".flow-states/test-feature-dag.md"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## DAG Analysis" in body
    assert "DAG from files block" in body


def test_artifacts_table_from_files_block(tmp_path):
    """Artifacts section shows files block as a table with relative paths."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    state["files"]["plan"] = ".flow-states/test-feature-plan.md"
    state["files"]["dag"] = ".flow-states/test-feature-dag.md"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "| File | Path |" in body
    assert ".flow-states/test-feature-plan.md" in body
    assert ".flow-states/test-feature-dag.md" in body
    assert ".flow-states/test-feature.log" in body
    assert ".flow-states/test-feature.json" in body


def test_legacy_artifacts_without_files_block(tmp_path):
    """State without files block uses legacy bullet format for artifacts."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    del state["files"]
    state["plan_file"] = "/abs/path/to/plan.md"
    state["dag_file"] = "/abs/path/to/dag.md"
    state["transcript_path"] = "/abs/path/to/session.jsonl"
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "**Plan file**" in body
    assert "**DAG file**" in body
    assert "**Session log**" in body
    assert "| File | Path |" not in body


def test_empty_artifacts_no_files_block(tmp_path):
    """State without files block and no artifact paths — empty Artifacts."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    del state["files"]
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## Artifacts\n\n## Phase" in body


def test_missing_plan_file(tmp_path):
    """Plan file path set but file missing — Plan section omitted."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    state["plan_file"] = str(tmp_path / "nonexistent-plan.md")
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## Plan" not in body


def test_missing_dag_file(tmp_path):
    """DAG file path set but file missing — DAG Analysis section omitted."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    state["dag_file"] = str(tmp_path / "nonexistent-dag.md")
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"

    body = _mod.render_body(state, tmp_path)

    assert "## DAG Analysis" not in body


def test_idempotent(tmp_path):
    """Two renders with same state produce identical output."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    plan_file = tmp_path / "plan.md"
    plan_file.write_text("Plan content")
    state["plan_file"] = str(plan_file)
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"

    body1 = _mod.render_body(state, tmp_path)
    body2 = _mod.render_body(state, tmp_path)

    assert body1 == body2


def test_phase_timings_shows_started_only(tmp_path):
    """Only phases with started_at or cumulative_seconds > 0 show rows."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["phases"]["flow-start"]["cumulative_seconds"] = 30
    state["phases"]["flow-plan"]["started_at"] = "2026-01-01T00:01:00Z"
    state["phases"]["flow-plan"]["cumulative_seconds"] = 300
    state["phases"]["flow-code"]["started_at"] = "2026-01-01T00:06:00Z"
    # Pending phases have no started_at and 0 seconds

    body = _mod.render_body(state, tmp_path)

    assert "| Start |" in body
    assert "| Plan |" in body
    assert "| Code |" in body
    # Pending phases should NOT appear
    assert "| Code Review |" not in body
    assert "| Learn |" not in body
    assert "| Complete |" not in body


def test_section_order(tmp_path):
    """Full state: sections appear in canonical order."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "complete",
            "flow-complete": "complete",
        },
    )
    for key in state["phases"]:
        state["phases"][key]["started_at"] = "2026-01-01T00:00:00Z"
        state["phases"][key]["cumulative_seconds"] = 60

    plan_file = tmp_path / "plan.md"
    plan_file.write_text("Plan")
    dag_file = tmp_path / "dag.md"
    dag_file.write_text("DAG")
    log_file = tmp_path / ".flow-states" / "test-feature.log"
    log_file.parent.mkdir(parents=True, exist_ok=True)
    log_file.write_text("log entry")
    state["plan_file"] = str(plan_file)
    state["dag_file"] = str(dag_file)
    state["transcript_path"] = "/path/to/session.jsonl"
    state["issues_filed"] = [
        {"label": "Flow", "title": "Issue", "url": "https://github.com/t/t/issues/1",
         "phase_name": "Learn"},
    ]

    body = _mod.render_body(state, tmp_path)

    headings = [
        "## What",
        "## Artifacts",
        "## Plan",
        "## DAG Analysis",
        "## Phase Timings",
        "## State File",
        "## Session Log",
        "## Issues Filed",
    ]
    positions = [body.index(h) for h in headings]
    assert positions == sorted(positions), (
        f"Sections out of order: {list(zip(headings, positions))}"
    )


def test_no_issues_no_section(tmp_path):
    """Empty issues_filed — Issues Filed section absent."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={"flow-start": "complete"},
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["issues_filed"] = []

    body = _mod.render_body(state, tmp_path)

    assert "## Issues Filed" not in body


# --- CLI integration ---


def test_cli_integration(target_project):
    """Subprocess call via bin/flow render-pr-body returns JSON."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["branch"] = "test-feature"
    state_file = state_dir / "test-feature.json"
    state_file.write_text(json.dumps(state))

    result = subprocess.run(
        [sys.executable, SCRIPT, "--pr", "1", "--state-file", str(state_file),
         "--dry-run"],
        capture_output=True, text=True, cwd=str(target_project),
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert "sections" in data
    assert "What" in data["sections"]


def test_cli_missing_state_file(tmp_path):
    """CLI returns error when state file does not exist."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--pr", "1",
         "--state-file", str(tmp_path / "nonexistent.json"), "--dry-run"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_gh_set_body_success():
    """_gh_set_body calls gh pr edit with correct args."""
    with patch.object(_mod.subprocess, "run") as mock_run:
        mock_run.return_value = MagicMock(returncode=0)
        _mod._gh_set_body(42, "body text")
    mock_run.assert_called_once_with(
        ["gh", "pr", "edit", "42", "--body", "body text"],
        capture_output=True, text=True,
    )


def test_gh_set_body_failure():
    """_gh_set_body raises RuntimeError on failure."""
    with patch.object(_mod.subprocess, "run") as mock_run:
        mock_run.return_value = MagicMock(returncode=1, stderr="auth failed", stdout="")
        with pytest.raises(RuntimeError, match="auth failed"):
            _mod._gh_set_body(42, "body")


def test_main_auto_detect_state_file(tmp_path):
    """main() auto-detects state file from project_root and current_branch."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state["branch"] = "my-branch"
    state_file = state_dir / "my-branch.json"
    state_file.write_text(json.dumps(state))

    with patch.object(_mod, "project_root", return_value=str(tmp_path)), \
         patch.object(_mod, "current_branch", return_value="my-branch"), \
         patch.object(_mod, "_gh_set_body"), \
         patch("sys.argv", ["render-pr-body", "--pr", "1"]):
        _mod.main()
    # If no exception, auto-detect worked


def test_main_non_dry_run_calls_gh(tmp_path):
    """main() in non-dry-run mode calls _gh_set_body."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["phases"]["flow-start"]["started_at"] = "2026-01-01T00:00:00Z"
    state_file = state_dir / "state.json"
    state_file.write_text(json.dumps(state))

    with patch.object(_mod, "_gh_set_body") as mock_gh, \
         patch("sys.argv", ["render-pr-body", "--pr", "99",
                            "--state-file", str(state_file)]):
        _mod.main()
    mock_gh.assert_called_once()
    assert mock_gh.call_args[0][0] == 99


def test_main_error_handling(tmp_path):
    """main() catches exceptions and returns error JSON."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state_file = state_dir / "state.json"
    state_file.write_text("invalid json {{{")

    import io
    captured = io.StringIO()
    with patch("sys.argv", ["render-pr-body", "--pr", "1",
                            "--state-file", str(state_file)]), \
         patch("sys.stdout", captured):
        _mod.main()
    data = json.loads(captured.getvalue())
    assert data["status"] == "error"
