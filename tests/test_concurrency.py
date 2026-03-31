"""Tests for concurrent access to FLOW's shared resources.

All tests use multiprocessing.Process for real process isolation.
Worker functions are module-level for pickling compatibility.
"""

import json
import multiprocessing
import os
import subprocess
import sys
import time
from pathlib import Path

LIB_DIR = str(Path(__file__).resolve().parent.parent / "lib")


def _init_git_repo(path):
    """Create a minimal git repo at path for project_root() resolution."""
    subprocess.run(
        ["git", "-c", "init.defaultBranch=main", "init"],
        cwd=str(path),
        capture_output=True,
        check=True,
    )
    config = path / ".git" / "config"
    with open(config, "a") as f:
        f.write("[user]\n\temail = t@t.com\n\tname = T\n[commit]\n\tgpgsign = false\n")
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=str(path),
        capture_output=True,
        check=True,
    )


# --- Worker functions (module-level for multiprocessing pickling) ---


def _worker_mutate_increment(state_path_str, lib_dir):
    """Increment counter in state file via mutate_state."""
    sys.path.insert(0, lib_dir)
    from flow_utils import mutate_state

    mutate_state(
        state_path_str,
        lambda s: s.__setitem__("count", s.get("count", 0) + 1),
    )


def _worker_log_append(repo_path_str, worker_id, lib_dir):
    """Append a unique line to log file via append_log."""
    sys.path.insert(0, lib_dir)
    os.chdir(repo_path_str)
    from log import append_log

    append_log("test-branch", f"worker-{worker_id}")


def _worker_start_lock(repo_path_str, worker_id, results_dir_str, lib_dir, delay):
    """Acquire lock, hold briefly, release. Record timing to file."""
    time.sleep(delay)
    sys.path.insert(0, lib_dir)
    os.chdir(repo_path_str)
    import importlib.util

    spec = importlib.util.spec_from_file_location("start_lock", os.path.join(lib_dir, "start-lock.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    result = mod.acquire_with_wait(
        f"feature-{worker_id}",
        timeout=30,
        interval=0.1,
    )
    acquired_at = time.monotonic()
    time.sleep(0.3)
    released_at = time.monotonic()
    mod.release(f"feature-{worker_id}")

    Path(results_dir_str, f"worker-{worker_id}.json").write_text(
        json.dumps(
            {
                "worker_id": worker_id,
                "status": result["status"],
                "acquired_at": acquired_at,
                "released_at": released_at,
            }
        )
    )


def _worker_create_state(state_dir_str, branch):
    """Create a state file for a branch."""
    path = Path(state_dir_str) / f"{branch}.json"
    state = {"branch": branch, "status": "created"}
    path.write_text(json.dumps(state, indent=2))


def _worker_cleanup(project_root_str, branch, worktree, lib_dir):
    """Run cleanup() on a branch."""
    sys.path.insert(0, lib_dir)
    import importlib.util

    spec = importlib.util.spec_from_file_location("cleanup", os.path.join(lib_dir, "cleanup.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.cleanup(project_root_str, branch, worktree)


def _worker_mutate_flag(state_path_str, lib_dir):
    """Set mutated=True in a state file via mutate_state."""
    sys.path.insert(0, lib_dir)
    from flow_utils import mutate_state

    mutate_state(
        state_path_str,
        lambda s: s.__setitem__("mutated", True),
    )


# --- Tests ---


def test_mutate_state_under_contention(tmp_path):
    """20 parallel workers increment a counter via mutate_state. Final = 20."""
    state_file = tmp_path / "shared.json"
    state_file.write_text(json.dumps({"count": 0}))

    workers = []
    for i in range(20):
        p = multiprocessing.Process(
            target=_worker_mutate_increment,
            args=(str(state_file), LIB_DIR),
        )
        workers.append(p)

    for p in workers:
        p.start()
    for p in workers:
        p.join(timeout=30)
        assert p.exitcode == 0, f"Worker exited with code {p.exitcode}"

    state = json.loads(state_file.read_text())
    assert state["count"] == 20


def test_log_append_under_contention(tmp_path):
    """20 parallel workers append unique lines. File has 20 non-corrupted lines."""
    _init_git_repo(tmp_path)
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    workers = []
    for i in range(20):
        p = multiprocessing.Process(
            target=_worker_log_append,
            args=(str(tmp_path), i, LIB_DIR),
        )
        workers.append(p)

    for p in workers:
        p.start()
    for p in workers:
        p.join(timeout=30)
        assert p.exitcode == 0, f"Worker exited with code {p.exitcode}"

    log_file = state_dir / "test-branch.log"
    assert log_file.exists()
    lines = log_file.read_text().strip().split("\n")
    assert len(lines) == 20

    # Each line should contain a unique worker-N marker
    markers = set()
    for line in lines:
        for part in line.split():
            if part.startswith("worker-"):
                markers.add(part)
    assert len(markers) == 20


def test_start_lock_serialization(tmp_path):
    """3 parallel workers acquire lock. No two hold it simultaneously."""
    _init_git_repo(tmp_path)
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    results_dir = tmp_path / "results"
    results_dir.mkdir()

    workers = []
    for i in range(3):
        p = multiprocessing.Process(
            target=_worker_start_lock,
            args=(str(tmp_path), i, str(results_dir), LIB_DIR, i * 0.1),
        )
        workers.append(p)

    for p in workers:
        p.start()
    for p in workers:
        p.join(timeout=30)
        assert p.exitcode == 0, f"Worker exited with code {p.exitcode}"

    timings = []
    for i in range(3):
        data = json.loads((results_dir / f"worker-{i}.json").read_text())
        assert data["status"] == "acquired"
        timings.append(data)

    # Sort by acquired_at and verify non-overlapping intervals
    timings.sort(key=lambda t: t["acquired_at"])
    for i in range(1, len(timings)):
        assert timings[i]["acquired_at"] >= timings[i - 1]["released_at"], (
            f"Worker {timings[i]['worker_id']} overlaps with worker {timings[i - 1]['worker_id']}"
        )


def test_thundering_herd_zero_delay(tmp_path):
    """5 workers start simultaneously (delay=0). All acquire, no overlaps."""
    _init_git_repo(tmp_path)
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    results_dir = tmp_path / "results"
    results_dir.mkdir()

    workers = []
    for i in range(5):
        p = multiprocessing.Process(
            target=_worker_start_lock,
            args=(str(tmp_path), i, str(results_dir), LIB_DIR, 0),
        )
        workers.append(p)

    for p in workers:
        p.start()
    for p in workers:
        p.join(timeout=60)
        assert p.exitcode == 0, f"Worker exited with code {p.exitcode}"

    timings = []
    for i in range(5):
        data = json.loads((results_dir / f"worker-{i}.json").read_text())
        assert data["status"] == "acquired", f"Worker {i} got status={data['status']}"
        timings.append(data)

    # Sort by acquired_at and verify non-overlapping intervals
    timings.sort(key=lambda t: t["acquired_at"])
    for i in range(1, len(timings)):
        assert timings[i]["acquired_at"] >= timings[i - 1]["released_at"], (
            f"Worker {timings[i]['worker_id']} overlaps with worker {timings[i - 1]['worker_id']}"
        )


def test_parallel_state_file_creation(tmp_path):
    """5 parallel workers create state files for different branches."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    branches = [f"branch-{i}" for i in range(5)]
    workers = []
    for branch in branches:
        p = multiprocessing.Process(
            target=_worker_create_state,
            args=(str(state_dir), branch),
        )
        workers.append(p)

    for p in workers:
        p.start()
    for p in workers:
        p.join(timeout=30)
        assert p.exitcode == 0, f"Worker exited with code {p.exitcode}"

    for branch in branches:
        state_file = state_dir / f"{branch}.json"
        assert state_file.exists()
        data = json.loads(state_file.read_text())
        assert data["branch"] == branch
        assert data["status"] == "created"


def test_cleanup_isolation(tmp_path):
    """cleanup() on branch-A does not affect branch-B's state file."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    state_a = state_dir / "branch-a.json"
    state_a.write_text(json.dumps({"branch": "branch-a", "count": 0}))
    state_b = state_dir / "branch-b.json"
    state_b.write_text(json.dumps({"branch": "branch-b", "count": 0}))

    p1 = multiprocessing.Process(
        target=_worker_cleanup,
        args=(str(tmp_path), "branch-a", ".worktrees/branch-a", LIB_DIR),
    )
    p2 = multiprocessing.Process(
        target=_worker_mutate_flag,
        args=(str(state_b), LIB_DIR),
    )

    p1.start()
    p2.start()
    p1.join(timeout=30)
    p2.join(timeout=30)
    assert p1.exitcode == 0
    assert p2.exitcode == 0

    # branch-a state file should be deleted by cleanup
    assert not state_a.exists()

    # branch-b state file should have the mutation
    data = json.loads(state_b.read_text())
    assert data["mutated"] is True
    assert data["branch"] == "branch-b"
