"""Tests for lib/tui_data.py — pure data layer for the interactive TUI."""

import json
from datetime import datetime

import tui_data
from conftest import make_orchestrate_state, make_state, write_state
from flow_utils import PACIFIC, PHASE_ORDER, elapsed_since, read_version, read_version_from

# --- load_all_flows ---


def test_load_all_flows_empty(state_dir):
    """Returns empty list when no state files exist."""
    result = tui_data.load_all_flows(state_dir.parent)
    assert result == []


def test_load_all_flows_single(state_dir):
    """Returns one flow summary for a single state file."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    write_state(state_dir, "test-feature", state)

    result = tui_data.load_all_flows(state_dir.parent)
    assert len(result) == 1
    assert result[0]["branch"] == "test-feature"


def test_load_all_flows_multiple(state_dir):
    """Returns multiple flow summaries sorted by feature name."""
    for name in ["charlie-feature", "alpha-feature", "bravo-feature"]:
        state = make_state()
        state["branch"] = name
        write_state(state_dir, name, state)

    result = tui_data.load_all_flows(state_dir.parent)
    assert len(result) == 3
    names = [flow["branch"] for flow in result]
    assert names == ["alpha-feature", "bravo-feature", "charlie-feature"]


def test_load_all_flows_skips_corrupt_json(state_dir):
    """Corrupt JSON files are skipped gracefully."""
    state = make_state()
    state["branch"] = "good-feature"
    write_state(state_dir, "good-feature", state)
    (state_dir / "bad-feature.json").write_text("{invalid json")

    result = tui_data.load_all_flows(state_dir.parent)
    assert len(result) == 1
    assert result[0]["branch"] == "good-feature"


def test_load_all_flows_skips_phases_json(state_dir):
    """Non-state JSON files like *-phases.json are excluded."""
    state = make_state()
    state["branch"] = "my-feature"
    write_state(state_dir, "my-feature", state)
    (state_dir / "my-feature-phases.json").write_text(json.dumps({"order": []}))

    result = tui_data.load_all_flows(state_dir.parent)
    assert len(result) == 1
    assert result[0]["branch"] == "my-feature"


def test_load_all_flows_no_state_dir(git_repo):
    """Returns empty list when .flow-states/ does not exist."""
    result = tui_data.load_all_flows(git_repo)
    assert result == []


# --- flow_summary ---


def test_flow_summary_basic():
    """Extracts basic display fields from a state dict."""
    now = datetime(2026, 1, 1, 1, 0, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    summary = tui_data.flow_summary(state, now=now)

    assert summary["feature"] == "Test Feature"
    assert summary["branch"] == "test-feature"
    assert summary["worktree"] == ".worktrees/test-feature"
    assert summary["pr_number"] == 1
    assert summary["pr_url"] == "https://github.com/test/test/pull/1"
    assert summary["phase_number"] == 3
    assert summary["phase_name"] == "Code"


def test_flow_summary_elapsed_time():
    """Elapsed time computed from started_at to now."""
    now = datetime(2026, 1, 1, 0, 42, 0, tzinfo=PACIFIC)
    state = make_state()
    state["started_at"] = "2026-01-01T00:00:00-08:00"
    summary = tui_data.flow_summary(state, now=now)

    assert summary["elapsed"] == "42m"


def test_flow_summary_code_task_present():
    """Extracts code_task when present."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    summary = tui_data.flow_summary(state)
    assert summary["code_task"] == 3


def test_flow_summary_code_task_absent():
    """code_task defaults to 0 when absent."""
    state = make_state()
    summary = tui_data.flow_summary(state)
    assert summary["code_task"] == 0


def test_flow_summary_diff_stats_present():
    """Extracts diff_stats when present."""
    state = make_state()
    state["diff_stats"] = {"files_changed": 5, "insertions": 100, "deletions": 20}
    summary = tui_data.flow_summary(state)
    assert summary["diff_stats"] == {"files_changed": 5, "insertions": 100, "deletions": 20}


def test_flow_summary_diff_stats_absent():
    """diff_stats defaults to None when absent."""
    state = make_state()
    summary = tui_data.flow_summary(state)
    assert summary["diff_stats"] is None


def test_flow_summary_notes_count():
    """Counts notes entries."""
    state = make_state()
    state["notes"] = [{"text": "note1"}, {"text": "note2"}]
    summary = tui_data.flow_summary(state)
    assert summary["notes_count"] == 2


def test_flow_summary_issues_count():
    """Counts issues_filed entries."""
    state = make_state()
    state["issues_filed"] = [{"url": "http://example.com/1"}]
    summary = tui_data.flow_summary(state)
    assert summary["issues_count"] == 1


def test_flow_summary_no_notes_or_issues():
    """Zero counts when notes and issues_filed are empty."""
    state = make_state()
    summary = tui_data.flow_summary(state)
    assert summary["notes_count"] == 0
    assert summary["issues_count"] == 0


def test_flow_summary_issues_populated():
    """issues key contains display-ready dicts from issues_filed."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "Extract helper for date parsing",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
        {
            "label": "Flaky Test",
            "title": "test_timeout flakes on CI",
            "url": "https://github.com/test/test/issues/55",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T11:00:00-08:00",
        },
    ]
    summary = tui_data.flow_summary(state)
    issues = summary["issues"]
    assert len(issues) == 2
    assert issues[0]["label"] == "Tech Debt"
    assert issues[0]["title"] == "Extract helper for date parsing"
    assert issues[0]["url"] == "https://github.com/test/test/issues/42"
    assert issues[0]["ref"] == "#42"
    assert issues[0]["phase_name"] == "Code Review"
    assert issues[1]["ref"] == "#55"


def test_flow_summary_issues_empty():
    """issues is empty list when issues_filed is empty."""
    state = make_state()
    state["issues_filed"] = []
    summary = tui_data.flow_summary(state)
    assert summary["issues"] == []


def test_flow_summary_issues_url_fallback():
    """ref falls back to full URL when URL is non-standard."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Flow",
            "title": "Process gap",
            "url": "https://example.com/custom/path",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T12:00:00-08:00",
        },
    ]
    summary = tui_data.flow_summary(state)
    assert summary["issues"][0]["ref"] == "https://example.com/custom/path"


def test_flow_summary_blocked_true():
    """State with _blocked set returns blocked: True."""
    state = make_state(current_phase="flow-code")
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    summary = tui_data.flow_summary(state)
    assert summary["blocked"] is True


def test_flow_summary_blocked_false():
    """State without _blocked returns blocked: False."""
    state = make_state(current_phase="flow-code")
    summary = tui_data.flow_summary(state)
    assert summary["blocked"] is False


def test_flow_summary_blocked_empty_string():
    """Empty string _blocked returns blocked: False."""
    state = make_state(current_phase="flow-code")
    state["_blocked"] = ""
    summary = tui_data.flow_summary(state)
    assert summary["blocked"] is False


def test_flow_summary_issue_numbers():
    """Extracts issue numbers from prompt."""
    state = make_state()
    state["prompt"] = "work on #83 and #89"
    summary = tui_data.flow_summary(state)
    assert summary["issue_numbers"] == {83, 89}


def test_flow_summary_plan_path_from_files():
    """Extracts plan_path from files.plan when set."""
    state = make_state()
    state["files"]["plan"] = ".flow-states/test-feature-plan.md"
    summary = tui_data.flow_summary(state)
    assert summary["plan_path"] == ".flow-states/test-feature-plan.md"


def test_flow_summary_plan_path_fallback_plan_file():
    """Falls back to plan_file when files.plan is None."""
    state = make_state()
    state["files"]["plan"] = None
    state["plan_file"] = ".flow-states/test-feature-plan.md"
    summary = tui_data.flow_summary(state)
    assert summary["plan_path"] == ".flow-states/test-feature-plan.md"


def test_flow_summary_plan_path_absent():
    """Returns None when both files.plan and plan_file are null."""
    state = make_state()
    summary = tui_data.flow_summary(state)
    assert summary["plan_path"] is None


# --- flow_summary: annotation ---


def test_flow_summary_annotation_code_phase():
    """annotation field contains active phase annotation for Code."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 2
    state["code_tasks_total"] = 5
    summary = tui_data.flow_summary(state)
    assert summary["annotation"] == "task 3 of 5"


def test_flow_summary_annotation_no_step_set():
    """annotation is empty when active phase has no step set."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    summary = tui_data.flow_summary(state)
    assert summary["annotation"] == ""


def test_flow_summary_annotation_start_phase():
    """annotation includes Start step tracking with step name."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["start_step"] = 5
    state["start_steps_total"] = 11
    summary = tui_data.flow_summary(state)
    assert summary["annotation"] == "pulling main - step 5 of 11"


# --- phase_timeline ---


def test_phase_timeline_all_pending():
    """All phases pending shows pending status."""
    state = make_state()
    timeline = tui_data.phase_timeline(state)
    assert len(timeline) == len(PHASE_ORDER)
    assert all(entry["status"] == "pending" for entry in timeline)


def test_phase_timeline_mixed():
    """Complete, in_progress, and pending all appear correctly."""
    now = datetime(2026, 1, 1, 0, 2, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 120
    state["phases"]["flow-plan"]["cumulative_seconds"] = 480
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 0

    timeline = tui_data.phase_timeline(state, now=now)

    assert timeline[0]["status"] == "complete"
    assert timeline[0]["time"] == "2m"
    assert timeline[0]["number"] == 1

    assert timeline[1]["status"] == "complete"
    assert timeline[1]["time"] == "8m"

    assert timeline[2]["status"] == "in_progress"
    assert timeline[2]["name"] == "Code"
    assert timeline[2]["time"] == "2m"

    assert timeline[3]["status"] == "pending"


# --- phase_timeline: Start ---


def test_phase_timeline_start_annotation():
    """Start phase shows step name with 'step 3 of 11' when start_step=3."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["start_step"] = 3
    state["start_steps_total"] = 11

    timeline = tui_data.phase_timeline(state)
    start_entry = timeline[0]
    assert start_entry["annotation"] == "creating state - step 3 of 11"
    assert start_entry["name"] == "Start", "step name lookup must not shadow the phase name"


def test_phase_timeline_start_step_zero():
    """Start phase has no annotation when start_step is 0."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )

    timeline = tui_data.phase_timeline(state)
    start_entry = timeline[0]
    assert start_entry["annotation"] == ""


def test_phase_timeline_start_no_total():
    """Start phase shows step name with 'step 3' when start_steps_total is absent."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["start_step"] = 3

    timeline = tui_data.phase_timeline(state)
    start_entry = timeline[0]
    assert start_entry["annotation"] == "creating state - step 3"


# --- phase_timeline: Plan ---


def test_phase_timeline_plan_annotation():
    """Plan phase shows step name with 'step 2 of 4' when plan_step=2."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["plan_step"] = 2
    state["plan_steps_total"] = 4

    timeline = tui_data.phase_timeline(state)
    plan_entry = timeline[1]
    assert plan_entry["annotation"] == "decomposing - step 2 of 4"


def test_phase_timeline_plan_step_zero():
    """Plan phase has no annotation when plan_step is 0."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )

    timeline = tui_data.phase_timeline(state)
    plan_entry = timeline[1]
    assert plan_entry["annotation"] == ""


def test_phase_timeline_plan_no_total():
    """Plan phase shows step name with 'step 2' when plan_steps_total is absent."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["plan_step"] = 2

    timeline = tui_data.phase_timeline(state)
    plan_entry = timeline[1]
    assert plan_entry["annotation"] == "decomposing - step 2"


# --- phase_timeline: Code ---


def test_phase_timeline_code_with_task_annotation():
    """Code phase shows current task (code_task + 1) with diff stats."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["diff_stats"] = {"files_changed": 5, "insertions": 127, "deletions": 48}

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert "task 4" in code_entry["annotation"]
    assert "+127" in code_entry["annotation"]
    assert "-48" in code_entry["annotation"]


def test_phase_timeline_code_first_task_annotation():
    """Code phase shows 'task 1 of 3' when code_task is 0 (working on first task)."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_tasks_total"] = 3
    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 1 of 3"


def test_phase_timeline_code_with_total():
    """Code phase shows 'task 4 of 8' when code_task=3 (working on task 4)."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["code_tasks_total"] = 8

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert "task 4 of 8" in code_entry["annotation"]


def test_phase_timeline_code_total_absent_fallback():
    """Falls back to 'task 4' when code_task=3 and code_tasks_total is absent."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 4"
    assert "of" not in code_entry["annotation"]


def test_phase_timeline_code_total_with_diff_stats():
    """Shows 'task 4 of 8, +127 -48' when code_task=3 and all fields present."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["code_tasks_total"] = 8
    state["diff_stats"] = {"insertions": 127, "deletions": 48}

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 4 of 8, +127 -48"


def test_phase_timeline_code_total_zero_ignored():
    """code_tasks_total=0 treated as absent (no 'of 0'), shows code_task+1."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["code_tasks_total"] = 0

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 4"
    assert "of" not in code_entry["annotation"]


# --- phase_timeline: Code overflow cap ---


def test_phase_timeline_code_task_overflow_capped():
    """When code_task == code_tasks_total, display shows total, not total+1."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["code_tasks_total"] = 3

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 3 of 3"


def test_phase_timeline_code_task_overflow_exceeds_total():
    """When code_task > code_tasks_total, display is capped at total."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 5
    state["code_tasks_total"] = 3

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 3 of 3"


# --- phase_timeline: Code task name ---


def test_phase_timeline_code_with_task_name():
    """Code phase shows 'name - task N of M' when code_task_name is present."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 1
    state["code_tasks_total"] = 3
    state["code_task_name"] = "Update contract tests"

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "Update contract tests - task 2 of 3"


def test_phase_timeline_code_task_name_absent():
    """Backward compat: no code_task_name in state shows plain 'task N of M'."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 1
    state["code_tasks_total"] = 3

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 2 of 3"


def test_phase_timeline_code_task_name_with_diff_stats():
    """Code phase shows 'name - task N of M, +X -Y' with task name and diff stats."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 1
    state["code_tasks_total"] = 3
    state["code_task_name"] = "Update contract tests"
    state["diff_stats"] = {"insertions": 127, "deletions": 48}

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "Update contract tests - task 2 of 3, +127 -48"


def test_phase_timeline_code_task_name_truncated():
    """Long task names are truncated with '...' at 30 chars."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 0
    state["code_tasks_total"] = 3
    state["code_task_name"] = "Implement the very long task description that exceeds limit"

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    name_part = code_entry["annotation"].split(" - task ")[0]
    assert len(name_part) == 30
    assert name_part.endswith("...")


def test_phase_timeline_code_task_name_empty_string():
    """Empty code_task_name falls back to plain 'task N of M'."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 1
    state["code_tasks_total"] = 3
    state["code_task_name"] = ""

    timeline = tui_data.phase_timeline(state)
    code_entry = timeline[2]
    assert code_entry["annotation"] == "task 2 of 3"


# --- phase_timeline: Code Review ---


def test_phase_timeline_code_review_step_zero():
    """Code Review phase shows 'simplifying - step 1 of 4' when code_review_step is 0."""
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    timeline = tui_data.phase_timeline(state)
    review_entry = timeline[3]
    assert review_entry["annotation"] == "simplifying - step 1 of 4"


def test_phase_timeline_code_review_annotation():
    """Code Review phase shows 'security review - step 3 of 4' when code_review_step=2."""
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    state["code_review_step"] = 2
    timeline = tui_data.phase_timeline(state)
    review_entry = timeline[3]
    assert review_entry["annotation"] == "security review - step 3 of 4"


def test_phase_timeline_code_review_complete():
    """Code Review phase has no annotation when code_review_step=4 (all done)."""
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    state["code_review_step"] = 4
    timeline = tui_data.phase_timeline(state)
    review_entry = timeline[3]
    assert review_entry["annotation"] == ""


def test_phase_timeline_code_review_step_four():
    """Code Review step 4 shows 'agent reviews - step 4 of 4' (code_review_step=3)."""
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    state["code_review_step"] = 3
    timeline = tui_data.phase_timeline(state)
    review_entry = timeline[3]
    assert review_entry["annotation"] == "agent reviews - step 4 of 4"


# --- phase_timeline: step name fallback ---


def test_phase_timeline_unknown_step_falls_back():
    """Step number with no name in mapping shows plain 'step N of M'."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state["start_step"] = 1
    state["start_steps_total"] = 11

    timeline = tui_data.phase_timeline(state)
    start_entry = timeline[0]
    assert start_entry["annotation"] == "step 1 of 11"


# --- phase_timeline: Learn ---


def test_phase_timeline_learn_annotation():
    """Learn phase shows 'committing - step 5 of 7' when learn_step=4."""
    state = make_state(
        current_phase="flow-learn",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "in_progress",
        },
    )
    state["learn_step"] = 4
    state["learn_steps_total"] = 7
    timeline = tui_data.phase_timeline(state)
    learn_entry = timeline[4]
    assert learn_entry["annotation"] == "committing - step 5 of 7"


def test_phase_timeline_learn_step_zero():
    """Learn phase shows 'gathering sources - step 1 of 7' when learn_step is 0."""
    state = make_state(
        current_phase="flow-learn",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "in_progress",
        },
    )
    state["learn_steps_total"] = 7
    timeline = tui_data.phase_timeline(state)
    learn_entry = timeline[4]
    assert learn_entry["annotation"] == "gathering sources - step 1 of 7"


# --- phase_timeline: Complete ---


def test_phase_timeline_complete_annotation():
    """Complete phase shows 'checking GitHub CI - step 5 of 12' when complete_step=5."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "complete",
            "flow-complete": "in_progress",
        },
    )
    state["complete_step"] = 5
    state["complete_steps_total"] = 12
    timeline = tui_data.phase_timeline(state)
    complete_entry = timeline[5]
    assert complete_entry["annotation"] == "checking GitHub CI - step 5 of 12"


def test_phase_timeline_complete_step_zero():
    """Complete phase has no annotation when complete_step is 0."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "complete",
            "flow-complete": "in_progress",
        },
    )
    state["complete_steps_total"] = 12
    timeline = tui_data.phase_timeline(state)
    complete_entry = timeline[5]
    assert complete_entry["annotation"] == ""


def test_phase_timeline_complete_step_one():
    """Complete phase shows 'checking state - step 1 of 12' when complete_step=1."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "complete",
            "flow-learn": "complete",
            "flow-complete": "in_progress",
        },
    )
    state["complete_step"] = 1
    state["complete_steps_total"] = 12
    timeline = tui_data.phase_timeline(state)
    complete_entry = timeline[5]
    assert complete_entry["annotation"] == "checking state - step 1 of 12"


# --- phase_timeline: live elapsed for in-progress ---


def test_phase_timeline_in_progress_live_time():
    """In-progress phase shows live elapsed from session_started_at."""
    now = datetime(2026, 1, 1, 0, 5, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 0

    timeline = tui_data.phase_timeline(state, now=now)
    code_entry = next(e for e in timeline if e["key"] == "flow-code")
    assert code_entry["time"] == "5m"


def test_phase_timeline_in_progress_cumulative_plus_live():
    """In-progress phase adds cumulative_seconds to live elapsed."""
    now = datetime(2026, 1, 1, 0, 3, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 120

    timeline = tui_data.phase_timeline(state, now=now)
    code_entry = next(e for e in timeline if e["key"] == "flow-code")
    assert code_entry["time"] == "5m"


def test_phase_timeline_in_progress_no_session_started():
    """In-progress phase with no session_started_at shows cumulative only."""
    now = datetime(2026, 1, 1, 0, 5, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["phases"]["flow-code"]["session_started_at"] = None
    state["phases"]["flow-code"]["cumulative_seconds"] = 60

    timeline = tui_data.phase_timeline(state, now=now)
    code_entry = next(e for e in timeline if e["key"] == "flow-code")
    assert code_entry["time"] == "1m"


# --- parse_log_entries ---


def test_parse_log_entries_basic():
    """Parses ISO timestamp + message format."""
    log = (
        "2026-01-01T10:15:00-08:00 [Phase 1] git worktree add (exit 0)\n"
        "2026-01-01T10:20:00-08:00 [Phase 2] Plan approved\n"
    )
    entries = tui_data.parse_log_entries(log)
    assert len(entries) == 2
    assert entries[0]["time"] == "10:15"
    assert entries[0]["message"] == "[Phase 1] git worktree add (exit 0)"
    assert entries[1]["time"] == "10:20"


def test_parse_log_entries_limit():
    """Returns only the last N entries when limit is set."""
    lines = [f"2026-01-01T10:{i:02d}:00-08:00 entry {i}\n" for i in range(30)]
    log = "".join(lines)
    entries = tui_data.parse_log_entries(log, limit=5)
    assert len(entries) == 5
    assert entries[0]["message"] == "entry 25"
    assert entries[4]["message"] == "entry 29"


def test_parse_log_entries_empty():
    """Returns empty list for empty string."""
    entries = tui_data.parse_log_entries("")
    assert entries == []


def test_parse_log_entries_none():
    """Returns empty list for None input."""
    entries = tui_data.parse_log_entries(None)
    assert entries == []


def test_parse_log_entries_malformed_lines():
    """Malformed lines are skipped."""
    log = (
        "2026-01-01T10:15:00-08:00 valid entry\n"
        "this line has no timestamp\n"
        "2026-01-01T10:20:00-08:00 another valid entry\n"
    )
    entries = tui_data.parse_log_entries(log)
    assert len(entries) == 2
    assert entries[0]["message"] == "valid entry"
    assert entries[1]["message"] == "another valid entry"


def test_parse_log_entries_blank_lines():
    """Blank lines in log content are skipped."""
    log = "2026-01-01T10:15:00-08:00 first entry\n\n2026-01-01T10:20:00-08:00 second entry\n"
    entries = tui_data.parse_log_entries(log)
    assert len(entries) == 2


def test_parse_log_entries_invalid_timestamp():
    """Lines with regex-matching but unparseable timestamps are skipped."""
    log = "9999-99-99T99:99:99-08:00 bad timestamp\n"
    entries = tui_data.parse_log_entries(log)
    assert entries == []


# --- elapsed_since ---


def test_elapsed_since_no_started_at():
    """Returns 0 when started_at is falsy."""
    assert elapsed_since(None) == 0
    assert elapsed_since("") == 0


def test_elapsed_since_default_now():
    """Uses current time when now is not passed."""
    result = elapsed_since("2026-01-01T00:00:00-08:00")
    assert isinstance(result, int)
    assert result >= 0


# --- read_version ---


def test_read_version_returns_string():
    """read_version returns a version string."""
    version = read_version()
    assert isinstance(version, str)
    assert version != ""
    # Should be a semver-like string or "?"
    assert "." in version or version == "?"


def test_read_version_missing_file(tmp_path):
    """Returns '?' when plugin.json is missing."""
    result = read_version_from(tmp_path / "nonexistent.json")
    assert result == "?"


# --- load_all_flows edge cases ---


def test_load_all_flows_skips_json_without_branch(state_dir):
    """JSON files without a 'branch' key are skipped."""
    (state_dir / "no-branch.json").write_text(json.dumps({"some": "data"}))
    state = make_state()
    state["branch"] = "real-feature"
    write_state(state_dir, "real-feature", state)

    result = tui_data.load_all_flows(state_dir.parent)
    assert len(result) == 1
    assert result[0]["branch"] == "real-feature"


# --- load_orchestration ---


def test_load_orchestration_no_file(state_dir):
    """Returns None when orchestrate.json does not exist."""
    result = tui_data.load_orchestration(state_dir.parent)
    assert result is None


def test_load_orchestration_with_state(state_dir):
    """Returns parsed state dict when orchestrate.json exists."""
    orch = make_orchestrate_state(
        queue=[
            {
                "issue_number": 42,
                "title": "Add PDF export",
                "status": "pending",
                "started_at": None,
                "completed_at": None,
                "outcome": None,
                "pr_url": None,
                "branch": None,
                "reason": None,
            },
        ]
    )
    (state_dir / "orchestrate.json").write_text(json.dumps(orch))

    result = tui_data.load_orchestration(state_dir.parent)
    assert result is not None
    assert result["started_at"] == "2026-03-20T22:00:00-07:00"
    assert len(result["queue"]) == 1


def test_load_orchestration_corrupt_json(state_dir):
    """Returns None on corrupt JSON."""
    (state_dir / "orchestrate.json").write_text("{corrupt json")
    result = tui_data.load_orchestration(state_dir.parent)
    assert result is None


def test_load_orchestration_no_state_dir(git_repo):
    """Returns None when .flow-states/ does not exist."""
    result = tui_data.load_orchestration(git_repo)
    assert result is None


# --- orchestration_summary ---


STATUS_ICONS = {
    "completed": "\u2713",
    "failed": "\u2717",
    "in_progress": "\u25b6",
    "pending": "\u00b7",
}


def _make_queue_item(
    issue_number,
    title,
    status="pending",
    started_at=None,
    completed_at=None,
    outcome=None,
    pr_url=None,
    branch=None,
    reason=None,
):
    """Build a queue item dict for tests."""
    return {
        "issue_number": issue_number,
        "title": title,
        "status": status,
        "started_at": started_at,
        "completed_at": completed_at,
        "outcome": outcome,
        "pr_url": pr_url,
        "branch": branch,
        "reason": reason,
    }


def test_orchestration_summary_no_state():
    """Returns None when state is None."""
    result = tui_data.orchestration_summary(None)
    assert result is None


def test_orchestration_summary_default_now():
    """Uses current time when now is not passed."""
    orch = make_orchestrate_state(queue=[])
    summary = tui_data.orchestration_summary(orch)
    assert summary is not None
    assert summary["total"] == 0


def test_orchestration_summary_basic():
    """Extracts queue items with status icons, elapsed, and counts."""
    now = datetime(2026, 3, 21, 0, 0, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(
        queue=[
            _make_queue_item(
                42,
                "Add PDF export",
                status="completed",
                outcome="completed",
                started_at="2026-03-20T22:00:00-07:00",
                completed_at="2026-03-20T23:24:00-07:00",
                pr_url="https://github.com/test/test/pull/58",
            ),
            _make_queue_item(43, "Fix login timeout", status="pending"),
        ]
    )

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["total"] == 2
    assert summary["completed_count"] == 1
    assert summary["failed_count"] == 0
    assert summary["is_running"] is True
    assert len(summary["items"]) == 2
    assert summary["items"][0]["icon"] == "\u2713"
    assert summary["items"][0]["issue_number"] == 42
    assert summary["items"][1]["icon"] == "\u00b7"


def test_orchestration_summary_with_completed_and_failed():
    """Correct counts for mixed outcomes."""
    now = datetime(2026, 3, 21, 2, 0, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(
        queue=[
            _make_queue_item(
                42,
                "A",
                status="completed",
                outcome="completed",
                started_at="2026-03-20T22:00:00-07:00",
                completed_at="2026-03-20T23:00:00-07:00",
            ),
            _make_queue_item(
                43,
                "B",
                status="failed",
                outcome="failed",
                started_at="2026-03-20T23:00:00-07:00",
                completed_at="2026-03-21T00:00:00-07:00",
                reason="CI failed after 3 attempts",
            ),
            _make_queue_item(44, "C", status="pending"),
        ]
    )

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["completed_count"] == 1
    assert summary["failed_count"] == 1
    assert summary["total"] == 3
    assert summary["items"][1]["icon"] == "\u2717"
    assert summary["items"][1]["reason"] == "CI failed after 3 attempts"


def test_orchestration_summary_in_progress_elapsed():
    """Live elapsed time for in-progress item."""
    now = datetime(2026, 3, 21, 0, 38, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(
        queue=[
            _make_queue_item(45, "Update hooks", status="in_progress", started_at="2026-03-21T00:00:00-07:00"),
        ],
        current_index=0,
    )

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["items"][0]["icon"] == "\u25b6"
    assert summary["items"][0]["elapsed"] == "38m"


def test_orchestration_summary_no_queue():
    """Handles empty queue."""
    now = datetime(2026, 3, 21, 0, 0, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(queue=[])

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["total"] == 0
    assert summary["items"] == []
    assert summary["is_running"] is True


def test_orchestration_summary_not_running():
    """Completed orchestration with completed_at set."""
    now = datetime(2026, 3, 21, 6, 0, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(
        queue=[
            _make_queue_item(
                42,
                "Done",
                status="completed",
                outcome="completed",
                started_at="2026-03-20T22:00:00-07:00",
                completed_at="2026-03-20T23:00:00-07:00",
            ),
        ],
        completed_at="2026-03-20T23:00:00-07:00",
    )

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["is_running"] is False
    assert summary["elapsed"] == "1h 0m"


def test_queue_item_display_icons():
    """Each status maps to the correct icon."""
    now = datetime(2026, 3, 21, 0, 0, 0, tzinfo=PACIFIC)
    orch = make_orchestrate_state(
        queue=[
            _make_queue_item(
                1,
                "A",
                status="completed",
                outcome="completed",
                started_at="2026-03-20T22:00:00-07:00",
                completed_at="2026-03-20T23:00:00-07:00",
            ),
            _make_queue_item(
                2,
                "B",
                status="failed",
                outcome="failed",
                started_at="2026-03-20T22:00:00-07:00",
                completed_at="2026-03-20T23:00:00-07:00",
            ),
            _make_queue_item(3, "C", status="in_progress", started_at="2026-03-20T23:00:00-07:00"),
            _make_queue_item(4, "D", status="pending"),
        ],
        current_index=2,
    )

    summary = tui_data.orchestration_summary(orch, now=now)

    assert summary["items"][0]["icon"] == "\u2713"
    assert summary["items"][1]["icon"] == "\u2717"
    assert summary["items"][2]["icon"] == "\u25b6"
    assert summary["items"][3]["icon"] == "\u00b7"
