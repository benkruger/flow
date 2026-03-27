"""Tests for lib/start-lock.py — serialize flow-start with file locking."""

import importlib
import json
import os
import sys
from datetime import datetime, timedelta
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("start-lock")


@pytest.fixture(autouse=True)
def _reset_lock_path_cache():
    """Reset _CACHED_LOCK_PATH between tests to prevent cache pollution."""
    yield
    if hasattr(_mod, "_CACHED_LOCK_PATH"):
        _mod._CACHED_LOCK_PATH = None


# --- acquire tests ---


def test_acquire_when_no_lock_exists(tmp_path):
    """Acquire creates lock file and returns acquired status."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("test-feature")

    assert result["status"] == "acquired"
    lock_file = state_dir / "start.lock"
    assert lock_file.exists()
    lock_data = json.loads(lock_file.read_text())
    assert lock_data["pid"] == 12345
    assert lock_data["feature"] == "test-feature"
    assert lock_data["acquired_at"] == "2026-01-01T10:00:00-08:00"


def test_acquire_when_locked_within_timeout(tmp_path):
    """Acquire returns locked status when lock exists and timeout not exceeded."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "other-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "other-feature"
    assert result["pid"] == 99999


def test_acquire_when_locked_by_dead_pid_within_timeout(tmp_path):
    """Acquire returns locked even when PID is dead — only timeout matters."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "dead-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "dead-feature"
    assert result["pid"] == 99999


def test_acquire_when_lock_exceeds_timeout(tmp_path):
    """Acquire breaks stale lock when it exceeds 30-minute timeout."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "old-feature",
                "acquired_at": "2026-01-01T08:00:00-08:00",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
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

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_lock_is_empty(tmp_path):
    """Acquire breaks lock when file is empty."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text("")

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_lock_has_missing_keys(tmp_path):
    """Acquire breaks lock when JSON is valid but missing required keys."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({"pid": 123}))  # missing feature, acquired_at

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_when_timestamp_unparseable(tmp_path):
    """Acquire breaks lock when acquired_at timestamp is invalid."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "other-feature",
                "acquired_at": "not-a-timestamp",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True


def test_acquire_creates_state_dir_if_missing(tmp_path):
    """Acquire creates .flow-states/ if it does not exist."""
    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert (tmp_path / ".flow-states" / "start.lock").exists()


def test_acquire_race_returns_locked(tmp_path):
    """When another process creates the lock between read and write, returns locked."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    # Pre-create lock file to simulate another process winning the race.
    winner_data = {"pid": 99999, "feature": "winner-feature", "acquired_at": "2026-01-01T10:00:00-08:00"}
    lock_file.write_text(json.dumps(winner_data))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(
            _mod,
            "_read_lock",
            side_effect=[
                (None, False),  # First call: appears free (TOCTOU window)
                (winner_data, True),  # Second call: sees the winner
            ],
        ),
    ):
        result = _mod.acquire("my-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "winner-feature"
    assert result["pid"] == 99999
    # Lock file must not be overwritten
    actual = json.loads(lock_file.read_text())
    assert actual["feature"] == "winner-feature"


def test_acquire_race_reread_also_fails(tmp_path):
    """When race-lost re-read also returns None, acquire returns locked with unknown."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    # Pre-create lock file so _try_write_lock fails.
    lock_file.write_text("temp")

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(
            _mod,
            "_read_lock",
            side_effect=[
                (None, False),  # First call: appears free
                (None, True),  # Second call: winner already released
            ],
        ),
    ):
        result = _mod.acquire("my-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "unknown"
    assert result["pid"] == 0


def test_break_and_acquire_race_returns_locked(tmp_path):
    """When another process wins the stale-break race, returns locked."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    winner_data = {"pid": 88888, "feature": "winner-feature", "acquired_at": "2026-01-01T10:01:00-08:00"}

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "_try_write_lock", return_value=None),
        patch.object(_mod, "_read_lock", return_value=(winner_data, True)),
    ):
        result = _mod._break_and_acquire(lock_file, "my-feature", "stale-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "winner-feature"
    assert result["pid"] == 88888


def test_break_and_acquire_race_reread_fails(tmp_path):
    """When stale-break race lost and re-read also fails, returns unknown."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "_try_write_lock", return_value=None),
        patch.object(_mod, "_read_lock", return_value=(None, False)),
    ):
        result = _mod._break_and_acquire(lock_file, "my-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "unknown"
    assert result["pid"] == 0


# --- _try_write_lock error paths ---


def test_try_write_lock_write_failure_closes_fd(tmp_path):
    """When os.write fails, the fd is still closed in the finally block."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    with (
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
        patch("os.write", side_effect=OSError("disk full")),
    ):
        result = _mod._try_write_lock(str(lock_file), "test-feature")

    assert result is None
    assert not lock_file.exists()


def test_try_write_lock_unlink_failure_ignored(tmp_path):
    """When os.unlink of the temp file fails, the error is silently ignored."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"

    original_unlink = os.unlink

    def fail_unlink(path):
        if ".start-lock-" in str(path):
            raise OSError("permission denied")
        return original_unlink(path)

    with (
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
        patch("os.unlink", side_effect=fail_unlink),
    ):
        result = _mod._try_write_lock(str(lock_file), "test-feature")

    assert result is not None
    assert result["feature"] == "test-feature"
    assert lock_file.exists()


# --- _lock_path tests ---


def test_lock_path_stable_across_cwd_changes(tmp_path):
    """_lock_path() returns the same absolute path even when project_root changes."""
    (tmp_path / ".flow-states").mkdir()
    other_dir = tmp_path / "other"
    (other_dir / ".flow-states").mkdir(parents=True)

    # Simulate project_root() returning different paths on successive calls
    # (e.g. Path(".") resolving differently after a cwd change).
    with patch.object(_mod, "project_root", side_effect=[tmp_path, other_dir]):
        path1 = _mod._lock_path()
        path2 = _mod._lock_path()

    assert path1 == path2
    assert path1.is_absolute()


def test_lock_path_absolute_when_project_root_returns_dot(tmp_path, monkeypatch):
    """_lock_path() returns an absolute path even when project_root() falls back to Path('.')."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / ".flow-states").mkdir()

    with patch.object(_mod, "project_root", return_value=Path(".")):
        result = _mod._lock_path()

    assert result.is_absolute()
    assert result == tmp_path / ".flow-states" / "start.lock"


# --- acquire_with_wait tests ---


def test_acquire_with_wait_immediate(tmp_path):
    """Wait mode acquires immediately when lock is free, no sleep called."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
        patch("os.getppid", return_value=12345),
        patch.object(_mod.time, "sleep") as mock_sleep,
    ):
        result = _mod.acquire_with_wait("test-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    mock_sleep.assert_not_called()


def test_acquire_with_wait_succeeds_after_retry(tmp_path):
    """Wait mode retries after sleep and succeeds when lock is released."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "other-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    sleep_args = []

    def mock_sleep_side_effect(seconds):
        sleep_args.append(seconds)
        lock_file.unlink()

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
        patch("os.getppid", return_value=12345),
        patch.object(_mod.time, "sleep", side_effect=mock_sleep_side_effect),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    assert sleep_args == [10]


def test_acquire_with_wait_timeout(tmp_path):
    """Wait mode returns timeout when lock is never released."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "blocking-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
        patch.object(_mod.time, "sleep"),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 35.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=30, interval=10)

    assert result["status"] == "timeout"
    assert result["feature"] == "blocking-feature"
    assert result["pid"] == 99999
    assert result["waited_seconds"] == 35


def test_acquire_with_wait_stale_during_wait(tmp_path):
    """Wait mode succeeds when timeout expires between retries."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "stale-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    now_calls = [
        "2026-01-01T10:05:00-08:00",  # First acquire: within timeout → locked
        "2026-01-01T10:35:00-08:00",  # Second acquire: timeout exceeded → break
        "2026-01-01T10:35:00-08:00",  # _try_write_lock: new lock timestamp
    ]

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", side_effect=now_calls),
        patch("os.getppid", return_value=12345),
        patch.object(_mod.time, "sleep"),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=300, interval=10)

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True
    assert result["stale_feature"] == "stale-feature"


def test_acquire_with_wait_timeout_zero(tmp_path):
    """Wait mode with timeout=0 makes a single attempt, no sleep."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(
        json.dumps(
            {
                "pid": 99999,
                "feature": "other-feature",
                "acquired_at": "2026-01-01T10:00:00-08:00",
            }
        )
    )

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
        patch.object(_mod.time, "sleep") as mock_sleep,
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0]),
    ):
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


def test_release_returns_error_when_lock_persists(tmp_path):
    """Release returns error when lock file still exists after unlink attempt."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({"pid": 1, "feature": "f", "acquired_at": "t"}))

    # Make unlink a no-op so the file persists
    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "unlink"),
    ):
        result = _mod.release()

    assert result["status"] == "error"
    assert "lock_path" in result
    assert lock_file.exists()


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


def test_check_when_dead_pid_within_timeout(tmp_path):
    """Check returns locked even when PID is dead — only timeout matters."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_data = {"pid": 99999, "feature": "dead-feature", "acquired_at": "2026-01-01T10:00:00-08:00"}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
    ):
        result = _mod.check()

    assert result["status"] == "locked"
    assert result["feature"] == "dead-feature"
    assert result["pid"] == 99999


def test_check_when_timed_out(tmp_path):
    """Check returns free when lock has exceeded the 30-minute timeout."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_data = {"pid": 99999, "feature": "old-feature", "acquired_at": "2026-01-01T08:00:00-08:00"}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:00:00-08:00"),
    ):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_when_locked(tmp_path):
    """Check returns lock details when lock file exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_data = {"pid": 55555, "feature": "some-feature", "acquired_at": "2026-01-01T10:00:00-08:00"}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod, "now", return_value="2026-01-01T10:05:00-08:00"),
    ):
        result = _mod.check()

    assert result["status"] == "locked"
    assert result["feature"] == "some-feature"
    assert result["pid"] == 55555


# --- real-clock stale detection ---


def test_check_stale_real_clock(tmp_path):
    """Check detects stale lock using real clock (no now() mock)."""
    from flow_utils import now

    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    # Create a lock timestamped 31 minutes in the past using real time
    real_now = datetime.fromisoformat(now())
    past = real_now - timedelta(seconds=_mod.STALE_TIMEOUT_SECONDS + 60)
    lock_data = {"pid": 99999, "feature": "old-feature", "acquired_at": past.isoformat()}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "free"


def test_acquire_stale_real_clock(tmp_path):
    """Acquire breaks stale lock using real clock (no now() mock)."""
    from flow_utils import now

    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    real_now = datetime.fromisoformat(now())
    past = real_now - timedelta(seconds=_mod.STALE_TIMEOUT_SECONDS + 60)
    lock_data = {"pid": 99999, "feature": "old-feature", "acquired_at": past.isoformat()}
    (state_dir / "start.lock").write_text(json.dumps(lock_data))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch("os.getppid", return_value=12345),
    ):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result["stale_broken"] is True
    assert result["stale_feature"] == "old-feature"


# --- CLI integration ---


def test_cli_acquire(target_project, monkeypatch, capsys):
    """CLI acquire creates lock file."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--acquire", "--feature", "cli-feature"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "acquired"
    assert (state_dir / "start.lock").exists()


def test_cli_release(target_project, monkeypatch, capsys):
    """CLI release deletes lock file."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()
    (state_dir / "start.lock").write_text(json.dumps({"pid": 1, "feature": "f", "acquired_at": "t"}))

    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--release"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "released"
    assert not (state_dir / "start.lock").exists()


def test_cli_check(target_project, monkeypatch, capsys):
    """CLI check returns status."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--check"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "free"


def test_cli_no_flags(target_project, monkeypatch, capsys):
    """CLI with no flags exits with error."""
    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


def test_cli_missing_feature_for_acquire(target_project, monkeypatch, capsys):
    """CLI acquire without --feature exits with error."""
    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--acquire"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


def test_cli_acquire_wait(target_project, monkeypatch, capsys):
    """CLI acquire with --wait acquires immediately when lock is free."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    monkeypatch.chdir(target_project)
    monkeypatch.setattr(
        "sys.argv",
        [
            "start-lock",
            "--acquire",
            "--wait",
            "--timeout",
            "1",
            "--feature",
            "cli-wait-feature",
        ],
    )
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "acquired"
    assert (state_dir / "start.lock").exists()
