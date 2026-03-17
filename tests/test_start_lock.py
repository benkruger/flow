"""Tests for lib/start-lock.py — serialize flow-start with file locking."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import call, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("start-lock")


# --- acquire tests ---


def test_acquire_when_no_lock_exists(tmp_path):
    """Acquire creates lock file and returns acquired status."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345):
        result = _mod.acquire("test-feature")

    assert result["status"] == "acquired"
    lock_file = state_dir / "start.lock"
    assert lock_file.exists()
    lock_data = json.loads(lock_file.read_text())
    assert lock_data["pid"] == 12345
    assert lock_data["feature"] == "test-feature"
    assert lock_data["acquired_at"] == "2026-01-01T10:00:00-08:00"


def test_acquire_when_locked_by_alive_pid(tmp_path):
    """Acquire returns locked status when another session holds the lock."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "other-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.kill") as mock_kill:
        mock_kill.return_value = None  # PID is alive
        result = _mod.acquire("new-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "other-feature"
    assert result["pid"] == 99999


def test_acquire_when_locked_by_dead_pid(tmp_path):
    """Acquire breaks stale lock when the holding PID is dead."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "dead-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch("os.kill", side_effect=ProcessLookupError):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True
    assert result["stale_feature"] == "dead-feature"
    lock_data = json.loads(lock_file.read_text())
    assert lock_data["pid"] == 12345
    assert lock_data["feature"] == "new-feature"


def test_acquire_when_lock_exceeds_timeout(tmp_path):
    """Acquire breaks stale lock when it exceeds 30-minute timeout."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "old-feature",
        "acquired_at": "2026-01-01T08:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch("os.kill") as mock_kill:
        mock_kill.return_value = None  # PID is alive, but timeout exceeded
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True
    assert result["stale_feature"] == "old-feature"


def test_acquire_when_lock_is_corrupted_json(tmp_path):
    """Acquire breaks lock when file contains invalid JSON."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text("{not valid json")

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_lock_is_empty(tmp_path):
    """Acquire breaks lock when file is empty."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text("")

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_lock_has_missing_keys(tmp_path):
    """Acquire breaks lock when JSON is valid but missing required keys."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({"pid": 123}))  # missing feature, acquired_at

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_pid_permission_error(tmp_path):
    """Acquire treats PermissionError on kill as alive (process exists)."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "other-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.kill", side_effect=PermissionError):
        result = _mod.acquire("new-feature")

    assert result["status"] == "locked"


def test_acquire_when_timestamp_unparseable(tmp_path):
    """Acquire breaks lock when acquired_at timestamp is invalid."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "other-feature",
        "acquired_at": "not-a-timestamp",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch("os.kill") as mock_kill:
        mock_kill.return_value = None  # PID alive, but timestamp broken
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_creates_state_dir_if_missing(tmp_path):
    """Acquire creates .flow-states/ if it does not exist."""
    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert (tmp_path / ".flow-states" / "start.lock").exists()


# --- acquire_with_wait tests ---


def test_acquire_with_wait_immediate(tmp_path):
    """Wait mode acquires immediately when lock is free, no sleep called."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch.object(_mod.time, "sleep") as mock_sleep:
        result = _mod.acquire_with_wait("test-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    mock_sleep.assert_not_called()


def test_acquire_with_wait_succeeds_after_retry(tmp_path):
    """Wait mode retries after sleep and succeeds when lock is released."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "other-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    call_count = 0

    def mock_sleep_side_effect(seconds):
        nonlocal call_count
        call_count += 1
        lock_file.unlink()

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch("os.kill") as mock_kill, \
         patch.object(_mod.time, "sleep", side_effect=mock_sleep_side_effect), \
         patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]):
        mock_kill.return_value = None
        result = _mod.acquire_with_wait("new-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    assert call_count == 1


def test_acquire_with_wait_timeout(tmp_path):
    """Wait mode returns timeout when lock is never released."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "blocking-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.kill") as mock_kill, \
         patch.object(_mod.time, "sleep"), \
         patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 35.0]):
        mock_kill.return_value = None
        result = _mod.acquire_with_wait("new-feature", timeout=30, interval=10)

    assert result["status"] == "timeout"
    assert result["feature"] == "blocking-feature"
    assert result["pid"] == 99999
    assert result["waited_seconds"] == 35


def test_acquire_with_wait_stale_during_wait(tmp_path):
    """Wait mode succeeds when PID dies between retries (stale detected)."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "dying-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    kill_calls = [None, ProcessLookupError]

    def kill_side_effect(pid, sig):
        effect = kill_calls.pop(0)
        if effect is not None:
            raise effect

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.getppid", return_value=12345), \
         patch("os.kill", side_effect=kill_side_effect), \
         patch.object(_mod.time, "sleep"), \
         patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]):
        result = _mod.acquire_with_wait("new-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True
    assert result["stale_feature"] == "dying-feature"


def test_acquire_with_wait_timeout_zero(tmp_path):
    """Wait mode with timeout=0 makes a single attempt, no sleep."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999,
        "feature": "other-feature",
        "acquired_at": "2026-01-01T10:00:00-08:00",
    }))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"), \
         patch("os.kill") as mock_kill, \
         patch.object(_mod.time, "sleep") as mock_sleep, \
         patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0]):
        mock_kill.return_value = None
        result = _mod.acquire_with_wait("new-feature", timeout=0, interval=10)

    assert result["status"] == "timeout"
    mock_sleep.assert_not_called()


# --- release tests ---


def test_release_deletes_lock_file(tmp_path):
    """Release deletes the lock file."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({"pid": 1, "feature": "f", "acquired_at": "t"}))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.release()

    assert result["status"] == "released"
    assert not lock_file.exists()


def test_release_idempotent_when_no_lock(tmp_path):
    """Release succeeds even when no lock file exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.release()

    assert result["status"] == "released"


# --- check tests ---


def test_check_when_free(tmp_path):
    """Check returns free when no lock file exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_when_dead_pid(tmp_path):
    """Check returns free when lock exists but PID is dead."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_data = {"pid": 99999, "feature": "dead-feature",
                 "acquired_at": "2026-01-01T10:00:00-08:00"}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch("os.kill", side_effect=ProcessLookupError):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_when_locked(tmp_path):
    """Check returns lock details when lock file exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_data = {"pid": 55555, "feature": "some-feature",
                 "acquired_at": "2026-01-01T10:00:00-08:00"}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with patch.object(_mod, "project_root", return_value=tmp_path), \
         patch("os.kill") as mock_kill:
        mock_kill.return_value = None  # PID alive
        result = _mod.check()

    assert result["status"] == "locked"
    assert result["feature"] == "some-feature"
    assert result["pid"] == 55555


# --- CLI integration ---


def test_cli_acquire(target_project):
    """CLI acquire creates lock file."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script), "--acquire", "--feature", "cli-feature"],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "acquired"
    assert (state_dir / "start.lock").exists()


def test_cli_release(target_project):
    """CLI release deletes lock file."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()
    (state_dir / "start.lock").write_text(
        json.dumps({"pid": 1, "feature": "f", "acquired_at": "t"})
    )

    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script), "--release"],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "released"
    assert not (state_dir / "start.lock").exists()


def test_cli_check(target_project):
    """CLI check returns status."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script), "--check"],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "free"


def test_cli_no_flags(target_project):
    """CLI with no flags exits with error."""
    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script)],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"


def test_cli_missing_feature_for_acquire(target_project):
    """CLI acquire without --feature exits with error."""
    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script), "--acquire"],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"


def test_cli_acquire_wait(target_project):
    """CLI acquire with --wait acquires immediately when lock is free."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    script = Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    result = subprocess.run(
        [sys.executable, str(script),
         "--acquire", "--wait", "--timeout", "1",
         "--feature", "cli-wait-feature"],
        capture_output=True, text=True,
        cwd=str(target_project),
    )

    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "acquired"
    assert (state_dir / "start.lock").exists()
