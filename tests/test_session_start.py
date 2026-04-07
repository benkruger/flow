"""Tests for hooks/session-start.sh — the SessionStart hook."""

import json
import subprocess

from conftest import HOOKS_DIR, REPO_ROOT, make_state, write_state

SCRIPT = str(HOOKS_DIR / "session-start.sh")
SOURCE_FILE = REPO_ROOT / "src" / "commands" / "session_context.rs"


def _run(git_repo):
    """Run session-start.sh inside the given git repo."""
    result = subprocess.run(
        ["bash", SCRIPT],
        capture_output=True,
        text=True,
        cwd=str(git_repo),
    )
    return result


def _switch(git_repo, branch_name):
    """Switch the test git repo to a named branch (for branch isolation)."""
    subprocess.run(
        ["git", "checkout", "-b", branch_name],
        cwd=str(git_repo),
        capture_output=True,
        check=True,
    )


# --- No features ---


def test_no_state_directory_exits_0_silent(git_repo):
    """No .flow-states/ directory → exits 0, no stdout."""
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_empty_state_directory_exits_0_silent(git_repo):
    """Empty state directory → exits 0, no stdout."""
    (git_repo / ".flow-states").mkdir(parents=True)
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


# --- Tab color tests ---


def test_flow_json_no_state_files_exits_0_no_stdout(git_repo):
    """.flow.json exists but no state files → exit 0, no stdout (color goes to tty)."""
    (git_repo / ".flow.json").write_text(json.dumps({"flow_version": "0.38.0"}))
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_flow_json_empty_state_dir_exits_0_no_stdout(git_repo):
    """.flow.json + empty .flow-states/ → exit 0, no stdout."""
    (git_repo / ".flow.json").write_text(json.dumps({"flow_version": "0.38.0"}))
    (git_repo / ".flow-states").mkdir(parents=True)
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_no_flow_json_no_state_files_exits_silently(git_repo):
    """No .flow.json and no state files → exit 0, no stdout (existing behavior)."""
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_active_flow_color_sequences_not_in_stdout(git_repo):
    """Color escape sequences must not appear in stdout (they go to /dev/tty)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    state["branch"] = "color-test"
    write_state(state_dir, "color-test", state)

    _switch(git_repo, "color-test")
    result = _run(git_repo)
    assert result.returncode == 0

    # No JSON output — session-context only writes tab colors
    assert result.stdout.strip() == "", "Session-context must produce no JSON output — tab colors only"

    # iTerm2 color escape sequences must not be in stdout
    assert "\033]6;1;bg;" not in result.stdout


# --- Tab-colors-only behavior ---


def test_state_files_present_exits_0_no_json(git_repo):
    """State files exist on matching branch → exit 0, no stdout (tab colors only)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == "", "Session-context must produce no JSON output — tab colors only"


def test_on_main_with_state_files_exits_0_no_json(git_repo):
    """On main with active feature state files → exit 0, no stdout (tab colors only)."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    state["branch"] = "some-feature"
    write_state(state_dir, "some-feature", state)

    # Stay on main — do NOT call _switch
    result = _run(git_repo)
    assert result.returncode == 0
    assert result.stdout.strip() == "", "On main with active flows: must produce no JSON output"


def test_state_files_not_mutated(git_repo):
    """Session-context must not mutate any state files."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(parents=True)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    state["_last_failure"] = {"type": "test", "message": "should survive", "timestamp": "2026-01-01T00:00:00-08:00"}
    state["compact_summary"] = "should survive"
    state["_blocked"] = "2026-01-01T00:00:00-08:00"
    write_state(state_dir, "my-feature", state)

    _switch(git_repo, "my-feature")
    original = json.loads((state_dir / "my-feature.json").read_text())
    _run(git_repo)
    after = json.loads((state_dir / "my-feature.json").read_text())

    assert original == after, "State file must not be mutated by session-context"


# --- Tombstone tests ---


def test_session_context_no_state_mutation():
    """Tombstone: removed in PR #938. Must not return.

    State mutation functions corrupted every active flow's state when
    a session opened on main.
    """
    content = SOURCE_FILE.read_text()
    assert "reset_interrupted" not in content, "reset_interrupted was removed in PR #938"
    assert "consume_last_failure" not in content, "consume_last_failure was removed in PR #938"
    assert "consume_compact_data" not in content, "consume_compact_data was removed in PR #938"


def test_session_context_no_context_injection():
    """Tombstone: removed in PR #938. Must not return.

    Context injection listed all feature branch names and NOTE_INSTRUCTION
    in sessions on main, causing flow-note to target random flows.
    """
    content = SOURCE_FILE.read_text()
    assert "NOTE_INSTRUCTION" not in content, "NOTE_INSTRUCTION was removed in PR #938"
    assert "build_single_feature_context" not in content, "build_single_feature_context was removed in PR #938"
    assert "build_multi_feature_context" not in content, "build_multi_feature_context was removed in PR #938"
