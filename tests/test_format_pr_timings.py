"""Tests for lib/format-pr-timings.py — phase timings markdown table generation."""

import importlib.util
import json

from conftest import LIB_DIR, make_state

# Import format-pr-timings.py for in-process unit tests
_spec = importlib.util.spec_from_file_location("format_pr_timings", LIB_DIR / "format-pr-timings.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- format_timings_table ---


def test_format_timings_table_all_complete():
    """All phases complete produces a full table with total row."""
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
    state["phases"]["flow-start"]["cumulative_seconds"] = 36
    state["phases"]["flow-plan"]["cumulative_seconds"] = 945
    state["phases"]["flow-code"]["cumulative_seconds"] = 328
    state["phases"]["flow-code-review"]["cumulative_seconds"] = 500
    state["phases"]["flow-learn"]["cumulative_seconds"] = 352
    state["phases"]["flow-complete"]["cumulative_seconds"] = 20

    result = _mod.format_timings_table(state)
    assert "| Phase | Duration |" in result
    assert "| Start |" in result
    assert "| Plan |" in result
    assert "| Code Review |" in result
    assert "| **Total** |" in result


def test_format_timings_table_partial_state():
    """Phases with zero seconds show <1m, pending phases still appear."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 30
    state["phases"]["flow-plan"]["cumulative_seconds"] = 600

    result = _mod.format_timings_table(state)
    assert "| Start |" in result
    assert "| Plan |" in result
    assert "| Code |" in result
    # Pending phases with 0 seconds should show <1m
    assert "| Complete |" in result


def test_format_timings_table_started_only():
    """started_only=True excludes phases with no started_at and 0 seconds."""
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

    result = _mod.format_timings_table(state, started_only=True)

    assert "| Start |" in result
    assert "| Plan |" in result
    assert "| Code |" in result
    assert "| Code Review |" not in result
    assert "| Learn |" not in result
    assert "| Complete |" not in result
    assert "| **Total** |" in result


def test_format_timings_table_uses_format_time():
    """Duration values use flow_utils.format_time formatting."""
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
    state["phases"]["flow-plan"]["cumulative_seconds"] = 3700

    result = _mod.format_timings_table(state)
    # 3700 seconds = 1h 1m
    assert "1h 1m" in result


# --- CLI end-to-end (in-process) ---


def test_cli_writes_output_file(tmp_path, monkeypatch, capsys):
    """CLI reads state file and writes markdown to output file."""
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
    state["phases"]["flow-start"]["cumulative_seconds"] = 60
    state["phases"]["flow-plan"]["cumulative_seconds"] = 300

    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps(state))
    output_file = tmp_path / "timings.md"

    monkeypatch.setattr(
        "sys.argv", ["format-pr-timings.py", "--state-file", str(state_file), "--output", str(output_file)]
    )

    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert output_file.exists()
    content = output_file.read_text()
    assert "| Phase | Duration |" in content


def test_cli_missing_state_file(tmp_path, monkeypatch, capsys):
    """CLI returns error when state file does not exist."""
    output_file = tmp_path / "timings.md"

    monkeypatch.setattr(
        "sys.argv",
        ["format-pr-timings.py", "--state-file", str(tmp_path / "nonexistent.json"), "--output", str(output_file)],
    )

    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_cli_invalid_json_returns_error(tmp_path, monkeypatch, capsys):
    """CLI returns error when state file contains invalid JSON."""
    state_file = tmp_path / "bad.json"
    state_file.write_text("not valid json {{{")
    output_file = tmp_path / "timings.md"

    monkeypatch.setattr(
        "sys.argv", ["format-pr-timings.py", "--state-file", str(state_file), "--output", str(output_file)]
    )

    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
