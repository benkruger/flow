"""Tests for start-lock — queue-based start lock serialization (Rust implementation)."""

import json
import os
import pathlib
import subprocess
import time

import pytest
from conftest import BIN_DIR

BIN_FLOW = str(BIN_DIR / "flow")


@pytest.fixture
def queue_dir(target_project):
    """Create the start-queue directory inside .flow-states/."""
    qd = target_project / ".flow-states" / "start-queue"
    qd.mkdir(parents=True, exist_ok=True)
    return qd


def _run(cwd, *args):
    """Run start-lock via bin/flow."""
    result = subprocess.run(
        [BIN_FLOW, "start-lock", *args],
        capture_output=True,
        text=True,
        cwd=str(cwd),
    )
    return result


# --- acquire tests ---


def test_cli_acquire(target_project):
    """CLI acquire creates queue entry."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    result = _run(target_project, "--acquire", "--feature", "cli-feature")
    assert result.returncode == 0, result.stderr
    output = json.loads(result.stdout)
    assert output["status"] == "acquired"
    assert (state_dir / "start-queue" / "cli-feature").exists()


def test_cli_acquire_locked_by_older(target_project, queue_dir):
    """CLI acquire returns locked when an older queue entry exists."""
    older = queue_dir / "alpha-feature"
    older.write_text("")
    os.utime(older, (time.time() - 10, time.time() - 10))

    result = _run(target_project, "--acquire", "--feature", "beta-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "locked"
    assert output["feature"] == "alpha-feature"
    assert (queue_dir / "beta-feature").exists()


def test_cli_acquire_stale_cleanup(target_project, queue_dir):
    """CLI acquire cleans up stale entries and acquires."""
    stale = queue_dir / "old-feature"
    stale.write_text("")
    os.utime(stale, (time.time() - 1860, time.time() - 1860))

    result = _run(target_project, "--acquire", "--feature", "new-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "acquired"
    assert output.get("stale_broken") is True
    assert not stale.exists()


# --- release tests ---


def test_cli_release(target_project, queue_dir):
    """CLI release deletes queue entry."""
    (queue_dir / "cli-feature").write_text("")

    result = _run(target_project, "--release", "--feature", "cli-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "released"
    assert not (queue_dir / "cli-feature").exists()


def test_cli_release_idempotent(target_project):
    """CLI release succeeds even when no queue entry exists."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)

    result = _run(target_project, "--release", "--feature", "nonexistent")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "released"


# --- check tests ---


def test_cli_check_free(target_project):
    """CLI check returns free when queue is empty."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)

    result = _run(target_project, "--check")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "free"


def test_cli_check_locked(target_project, queue_dir):
    """CLI check returns locked when queue has entries."""
    (queue_dir / "some-feature").write_text("")

    result = _run(target_project, "--check")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "locked"
    assert output["feature"] == "some-feature"


# --- wait mode tests ---


def test_cli_acquire_wait_immediate(target_project):
    """CLI acquire with --wait acquires immediately when queue is empty."""
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)

    result = _run(
        target_project,
        "--acquire",
        "--wait",
        "--timeout",
        "1",
        "--feature",
        "cli-wait-feature",
    )
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "acquired"
    assert (state_dir / "start-queue" / "cli-wait-feature").exists()


# --- error cases ---


def test_cli_no_flags(target_project):
    """CLI with no action flags exits with error."""
    result = _run(target_project)
    assert result.returncode != 0


def test_cli_missing_feature_for_acquire(target_project):
    """CLI acquire without --feature exits with error."""
    result = _run(target_project, "--acquire")
    assert result.returncode != 0


def test_cli_missing_feature_for_release(target_project):
    """CLI release without --feature exits with error."""
    result = _run(target_project, "--release")
    assert result.returncode != 0


# --- tombstone tests ---


def test_no_python_start_lock():
    """Tombstone: start-lock.py removed in PR #809, ported to Rust. Must not return."""
    source = pathlib.Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    assert not source.exists(), "lib/start-lock.py was removed — start-lock is now in Rust"


def test_no_start_lock_filename():
    """Tombstone: start.lock replaced by start-queue/ in PR #715. Must not return."""
    source = pathlib.Path(__file__).resolve().parent.parent / "lib" / "start-lock.py"
    if source.exists():
        content = source.read_text()
        assert "start.lock" not in content, "start.lock was removed in PR #715 — use start-queue/ directory instead"
