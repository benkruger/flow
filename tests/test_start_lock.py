"""Tests for lib/start-lock.py — queue-based start lock serialization."""

import importlib
import json
import os
import sys
import time
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("start-lock")


@pytest.fixture(autouse=True)
def _reset_queue_path_cache():
    """Reset queue path cache between tests to prevent cache pollution."""
    yield
    # Reset both old and new cache names for transition safety
    for attr in ("_CACHED_QUEUE_PATH", "_CACHED_LOCK_PATH"):
        if hasattr(_mod, attr):
            setattr(_mod, attr, None)


# --- acquire tests ---


def test_acquire_when_no_lock_exists(tmp_path):
    """Acquire creates queue entry and returns acquired status."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("test-feature")

    assert result["status"] == "acquired"
    queue_dir = state_dir / "start-queue"
    assert queue_dir.is_dir()
    assert (queue_dir / "test-feature").exists()
    assert result["lock_path"] == str(queue_dir)


def test_acquire_when_locked_by_older_entry(tmp_path):
    """Acquire returns locked when an older queue entry exists."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    older = queue_dir / "alpha-feature"
    older.write_text("")
    os.utime(older, (time.time() - 10, time.time() - 10))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("beta-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "alpha-feature"
    assert result["lock_path"] == str(queue_dir)
    assert (queue_dir / "beta-feature").exists()


def test_acquire_when_lock_exceeds_timeout(tmp_path):
    """Acquire removes stale entries and acquires when only stale entries exist."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    stale = queue_dir / "old-feature"
    stale.write_text("")
    os.utime(stale, (time.time() - 1860, time.time() - 1860))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    assert result.get("stale_broken") is True
    assert not stale.exists()
    assert (queue_dir / "new-feature").exists()


def test_acquire_tiebreaker_by_feature_name(tmp_path):
    """When mtimes match, alphabetically first feature wins."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    same_time = time.time() - 10
    for name in ["charlie-feature", "alpha-feature"]:
        f = queue_dir / name
        f.write_text("")
        os.utime(f, (same_time, same_time))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("delta-feature")

    assert result["status"] == "locked"
    assert result["feature"] == "alpha-feature"


def test_acquire_creates_queue_dir_if_missing(tmp_path):
    """Acquire creates start-queue/ and .flow-states/ if they do not exist."""
    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("new-feature")

    assert result["status"] == "acquired"
    queue_dir = tmp_path / ".flow-states" / "start-queue"
    assert queue_dir.is_dir()
    assert (queue_dir / "new-feature").exists()


def test_acquire_idempotent_when_already_first(tmp_path):
    """Calling acquire again when already first returns acquired."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "my-feature").write_text("")

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("my-feature")

    assert result["status"] == "acquired"


def test_acquire_skips_non_file_entries(tmp_path):
    """Acquire ignores subdirectories in the queue."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "subdir").mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("my-feature")

    assert result["status"] == "acquired"


def test_acquire_handles_stat_failure(tmp_path):
    """Acquire skips entries that vanish between iterdir and stat."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    # Create a file that will fail stat during _list_queue
    (queue_dir / "vanishing-feature").write_text("")

    real_stat = Path.stat

    def flaky_stat(self):
        if self.name == "vanishing-feature":
            raise OSError("file vanished")
        return real_stat(self)

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "stat", flaky_stat),
    ):
        result = _mod.acquire("my-feature")

    assert result["status"] == "acquired"


def test_acquire_handles_iterdir_failure(tmp_path):
    """Acquire returns acquired when iterdir fails (defensive branch)."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    call_count = 0
    real_iterdir = Path.iterdir

    def failing_iterdir(self):
        nonlocal call_count
        if self == queue_dir:
            call_count += 1
            if call_count > 0:
                raise OSError("permission denied")
        return real_iterdir(self)

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "iterdir", failing_iterdir),
    ):
        result = _mod.acquire("my-feature")

    assert result["status"] == "acquired"


def test_acquire_stale_cleanup_preserves_fresh(tmp_path):
    """Stale cleanup removes old entries but preserves fresh ones."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    stale = queue_dir / "aaa-stale"
    stale.write_text("")
    os.utime(stale, (time.time() - 1860, time.time() - 1860))

    fresh = queue_dir / "bbb-fresh"
    fresh.write_text("")
    os.utime(fresh, (time.time() - 10, time.time() - 10))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.acquire("ccc-new")

    assert result["status"] == "locked"
    assert result["feature"] == "bbb-fresh"
    assert result.get("stale_broken") is True
    assert not stale.exists()
    assert fresh.exists()
    assert (queue_dir / "ccc-new").exists()


# --- _queue_path tests ---


def test_queue_path_stable_across_cwd_changes(tmp_path):
    """_queue_path() returns the same absolute path even when project_root changes."""
    (tmp_path / ".flow-states").mkdir()
    other_dir = tmp_path / "other"
    (other_dir / ".flow-states").mkdir(parents=True)

    with patch.object(_mod, "project_root", side_effect=[tmp_path, other_dir]):
        path1 = _mod._queue_path()
        path2 = _mod._queue_path()

    assert path1 == path2
    assert path1.is_absolute()


def test_queue_path_absolute_when_project_root_returns_dot(tmp_path, monkeypatch):
    """_queue_path() returns an absolute path even when project_root() returns Path('.')."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / ".flow-states").mkdir()

    with patch.object(_mod, "project_root", return_value=Path(".")):
        result = _mod._queue_path()

    assert result.is_absolute()
    assert result == tmp_path / ".flow-states" / "start-queue"


# --- acquire_with_wait tests ---


def test_acquire_with_wait_immediate(tmp_path):
    """Wait mode acquires immediately when queue is empty, no sleep called."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod.time, "sleep") as mock_sleep,
    ):
        result = _mod.acquire_with_wait("test-feature", timeout=90, interval=10)

    assert result["status"] == "acquired"
    mock_sleep.assert_not_called()


def test_acquire_with_wait_succeeds_after_retry(tmp_path):
    """Wait mode retries after sleep and succeeds when holder releases."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    older = queue_dir / "blocking-feature"
    older.write_text("")
    os.utime(older, (time.time() - 10, time.time() - 10))

    def mock_sleep_side_effect(seconds):
        older.unlink()

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod.time, "sleep", side_effect=mock_sleep_side_effect),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=90, interval=10)

    assert result["status"] == "acquired"


def test_acquire_with_wait_timeout(tmp_path):
    """Wait mode returns timeout when lock is never released."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    older = queue_dir / "blocking-feature"
    older.write_text("")
    os.utime(older, (time.time() - 10, time.time() - 10))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod.time, "sleep"),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 35.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=30, interval=10)

    assert result["status"] == "timeout"
    assert result["feature"] == "blocking-feature"
    assert result["waited_seconds"] == 35


def test_acquire_with_wait_stale_during_wait(tmp_path):
    """Wait mode succeeds when stale timeout expires between retries."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    older = queue_dir / "stale-feature"
    older.write_text("")
    os.utime(older, (time.time() - 1740, time.time() - 1740))

    def mock_sleep_side_effect(seconds):
        os.utime(older, (time.time() - 1860, time.time() - 1860))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod.time, "sleep", side_effect=mock_sleep_side_effect),
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0, 10.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=90, interval=10)

    assert result["status"] == "acquired"
    assert result.get("stale_broken") is True


def test_acquire_with_wait_timeout_zero(tmp_path):
    """Wait mode with timeout=0 makes a single attempt, no sleep."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    older = queue_dir / "blocking-feature"
    older.write_text("")
    os.utime(older, (time.time() - 10, time.time() - 10))

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(_mod.time, "sleep") as mock_sleep,
        patch.object(_mod.time, "monotonic", side_effect=[0.0, 0.0]),
    ):
        result = _mod.acquire_with_wait("new-feature", timeout=0, interval=10)

    assert result["status"] == "timeout"
    mock_sleep.assert_not_called()


# --- release tests ---


def test_release_deletes_own_file(tmp_path):
    """Release deletes the feature's queue entry."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "my-feature").write_text("")

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.release("my-feature")

    assert result["status"] == "released"
    assert result["lock_path"] == str(queue_dir)
    assert not (queue_dir / "my-feature").exists()


def test_release_only_deletes_own_file(tmp_path):
    """Release only deletes the specified feature, other entries stay."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "my-feature").write_text("")
    (queue_dir / "other-feature").write_text("")

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.release("my-feature")

    assert result["status"] == "released"
    assert not (queue_dir / "my-feature").exists()
    assert (queue_dir / "other-feature").exists()


def test_release_idempotent_when_no_file(tmp_path):
    """Release succeeds even when no queue entry exists for the feature."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.release("nonexistent-feature")

    assert result["status"] == "released"


def test_release_returns_error_when_file_persists(tmp_path):
    """Release returns error when queue entry still exists after unlink attempt."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    entry = queue_dir / "stubborn-feature"
    entry.write_text("")

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "unlink"),
    ):
        result = _mod.release("stubborn-feature")

    assert result["status"] == "error"
    assert "lock_path" in result


# --- check tests ---


def test_check_when_free(tmp_path):
    """Check returns free when queue is empty."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "free"
    assert result["lock_path"] == str(queue_dir)


def test_check_when_locked(tmp_path):
    """Check returns lock details when queue has entries."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "some-feature").write_text("")

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "locked"
    assert result["feature"] == "some-feature"


def test_check_stale_returns_free(tmp_path):
    """Check returns free when all queue entries are stale."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    stale = queue_dir / "old-feature"
    stale.write_text("")
    os.utime(stale, (time.time() - 1860, time.time() - 1860))

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_skips_non_file_entries(tmp_path):
    """Check ignores subdirectories in the queue."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "subdir").mkdir()

    with patch.object(_mod, "project_root", return_value=tmp_path):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_handles_stat_failure(tmp_path):
    """Check skips entries that vanish between iterdir and stat."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "vanishing-feature").write_text("")

    real_stat = Path.stat

    def flaky_stat(self):
        if self.name == "vanishing-feature":
            raise OSError("file vanished")
        return real_stat(self)

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "stat", flaky_stat),
    ):
        result = _mod.check()

    assert result["status"] == "free"


def test_check_handles_iterdir_failure(tmp_path):
    """Check returns free when iterdir raises OSError."""
    state_dir = tmp_path / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)

    real_iterdir = Path.iterdir

    def failing_iterdir(self):
        if self == queue_dir:
            raise OSError("permission denied")
        return real_iterdir(self)

    with (
        patch.object(_mod, "project_root", return_value=tmp_path),
        patch.object(Path, "iterdir", failing_iterdir),
    ):
        result = _mod.check()

    assert result["status"] == "free"


# --- CLI integration ---


def test_cli_acquire(target_project, monkeypatch, capsys):
    """CLI acquire creates queue entry."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir()

    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--acquire", "--feature", "cli-feature"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "acquired"
    assert (state_dir / "start-queue" / "cli-feature").exists()


def test_cli_release(target_project, monkeypatch, capsys):
    """CLI release deletes queue entry."""
    state_dir = target_project / ".flow-states"
    queue_dir = state_dir / "start-queue"
    queue_dir.mkdir(parents=True)
    (queue_dir / "cli-feature").write_text("")

    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--release", "--feature", "cli-feature"])
    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "released"
    assert not (queue_dir / "cli-feature").exists()


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


def test_cli_missing_feature_for_release(target_project, monkeypatch, capsys):
    """CLI release without --feature exits with error."""
    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["start-lock", "--release"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


def test_cli_acquire_wait(target_project, monkeypatch, capsys):
    """CLI acquire with --wait acquires immediately when queue is empty."""
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
    assert (state_dir / "start-queue" / "cli-wait-feature").exists()
