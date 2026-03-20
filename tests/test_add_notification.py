"""Tests for lib/add-notification.py — records Slack notifications in the state file."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "add-notification.py")


def _import_module():
    """Import add-notification.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "add_notification", LIB_DIR / "add-notification.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _run(args, cwd=None, branch=None):
    """Run add-notification.py via subprocess."""
    cmd = [sys.executable, SCRIPT] + args
    if branch is not None:
        cmd.extend(["--branch", branch])
    return subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(cwd) if cwd else None,
    )


def _get_branch(git_repo):
    """Get the current branch name from a git repo."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result.stdout.strip()


# --- In-process tests ---


def test_append_to_empty_notifications(tmp_path):
    """add_notification creates slack_notifications array and appends first entry."""
    mod = _import_module()
    state = make_state(current_phase="flow-start", phase_statuses={
        "flow-start": "in_progress",
    })
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_notification(
        state_path, "flow-start", "1234567890.123456",
        "1234567890.123456", "Feature started",
    )

    assert len(updated["slack_notifications"]) == 1
    notif = updated["slack_notifications"][0]
    assert notif["phase"] == "flow-start"
    assert notif["phase_name"] == "Start"
    assert notif["ts"] == "1234567890.123456"
    assert notif["thread_ts"] == "1234567890.123456"
    assert notif["message_preview"] == "Feature started"
    assert "T" in notif["timestamp"]


def test_append_to_existing_notifications(tmp_path):
    """add_notification preserves existing entries and appends new one."""
    mod = _import_module()
    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    state["slack_notifications"] = [{
        "phase": "flow-start",
        "phase_name": "Start",
        "ts": "1111111111.111111",
        "thread_ts": "1111111111.111111",
        "message_preview": "Started",
        "timestamp": "2026-01-01T00:00:00-08:00",
    }]
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_notification(
        state_path, "flow-plan", "2222222222.222222",
        "1111111111.111111", "Plan complete",
    )

    assert len(updated["slack_notifications"]) == 2
    assert updated["slack_notifications"][0]["phase"] == "flow-start"
    assert updated["slack_notifications"][1]["phase"] == "flow-plan"


def test_creates_array_if_missing(tmp_path):
    """add_notification creates slack_notifications key if state file lacks it."""
    mod = _import_module()
    state = {"branch": "test", "current_phase": "flow-code"}
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_notification(
        state_path, "flow-code", "3333333333.333333",
        "1111111111.111111", "Task 1/5 complete",
    )

    assert len(updated["slack_notifications"]) == 1


def test_persists_to_disk(tmp_path):
    """add_notification writes the updated state back to disk."""
    mod = _import_module()
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    mod.add_notification(
        state_path, "flow-code", "4444444444.444444",
        "1111111111.111111", "Task complete",
    )

    on_disk = json.loads(state_path.read_text())
    assert len(on_disk["slack_notifications"]) == 1
    assert on_disk["slack_notifications"][0]["ts"] == "4444444444.444444"


def test_truncates_long_message_preview(tmp_path):
    """message_preview is truncated to 100 characters."""
    mod = _import_module()
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    long_message = "x" * 200
    updated = mod.add_notification(
        state_path, "flow-code", "5555555555.555555",
        "1111111111.111111", long_message,
    )

    preview = updated["slack_notifications"][0]["message_preview"]
    assert len(preview) <= 103  # 100 + "..."
    assert preview.endswith("...")


# --- CLI behavior (subprocess) ---


def test_cli_no_branch_returns_error(tmp_path):
    """Running outside a git repo returns an error."""
    result = _run([
        "--phase", "flow-start", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=tmp_path)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


def test_cli_no_state_file_returns_no_state(git_repo):
    """Running with no state file returns no_state."""
    result = _run([
        "--phase", "flow-start", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "no_state"


def test_cli_happy_path(state_dir, git_repo):
    """Full CLI round-trip: write state, run CLI, verify output."""
    branch_name = _get_branch(git_repo)
    state = make_state(current_phase="flow-start", phase_statuses={
        "flow-start": "in_progress",
    })
    write_state(state_dir, branch_name, state)

    result = _run([
        "--phase", "flow-start", "--ts", "1234567890.123456",
        "--thread-ts", "1234567890.123456", "--message", "Feature started",
    ], cwd=git_repo)

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["notification_count"] == 1


def test_cli_branch_flag(state_dir, git_repo):
    """--branch flag finds the state file for a different branch."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    write_state(state_dir, "other-feature", state)

    result = _run([
        "--phase", "flow-code", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=git_repo, branch="other-feature")

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"


def test_cli_ambiguous_multiple_state_files(state_dir, git_repo):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "in_progress",
        })
        write_state(state_dir, name, state)

    result = _run([
        "--phase", "flow-code", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=git_repo)

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Multiple" in data["message"]


def test_cli_write_failure_returns_error(state_dir, git_repo):
    """Read-only state file returns a write error."""
    branch_name = _get_branch(git_repo)
    state = make_state(current_phase="flow-start", phase_statuses={
        "flow-start": "in_progress",
    })
    path = write_state(state_dir, branch_name, state)
    path.chmod(0o444)

    result = _run([
        "--phase", "flow-start", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=git_repo)

    path.chmod(0o644)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


def test_cli_corrupt_state_returns_error(state_dir, git_repo):
    """Corrupt state file returns a read error."""
    branch_name = _get_branch(git_repo)
    bad_file = state_dir / f"{branch_name}.json"
    bad_file.write_text("{bad json")

    result = _run([
        "--phase", "flow-start", "--ts", "111.111",
        "--thread-ts", "111.111", "--message", "test",
    ], cwd=git_repo)

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
