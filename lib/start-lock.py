"""Serialize flow-start with file locking.

Prevents concurrent starts from fighting over main (CI fixes, dependency
updates). Only one flow-start runs at a time. Second start waits or
reports the lock.

Usage:
    bin/flow start-lock --acquire --feature <name>
    bin/flow start-lock --acquire --wait --feature <name>
    bin/flow start-lock --acquire --wait --timeout 300 --feature <name>
    bin/flow start-lock --release
    bin/flow start-lock --check

Output (JSON to stdout):
    Acquire: {"status": "acquired"} or {"status": "locked", "feature": ..., "pid": ..., "acquired_at": ...}
    Acquire --wait: {"status": "acquired"} or {"status": "timeout", "feature": ..., "pid": ..., "waited_seconds": ...}
    Release: {"status": "released"}
    Check:   {"status": "free"} or {"status": "locked", ...}
"""

import argparse
import json
import os
import sys
import time
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, project_root

LOCK_FILENAME = "start.lock"
STALE_TIMEOUT_SECONDS = 1800  # 30 minutes


def _lock_path():
    """Return the path to the lock file."""
    state_dir = project_root() / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    return state_dir / LOCK_FILENAME


def _read_lock(lock_file):
    """Read and parse lock file. Returns (data, file_existed).

    data is None if missing, empty, or corrupted. file_existed is True
    if the lock file was present on disk (even if corrupted).
    """
    if not lock_file.exists():
        return None, False
    try:
        text = lock_file.read_text().strip()
        if not text:
            return None, True
        data = json.loads(text)
        if "pid" not in data or "feature" not in data or "acquired_at" not in data:
            return None, True
        return data, True
    except (json.JSONDecodeError, OSError):
        return None, True


def _is_timed_out(acquired_at):
    """Check if the lock has exceeded the stale timeout."""
    try:
        lock_time = datetime.fromisoformat(acquired_at)
        current_time = datetime.fromisoformat(now())
        diff = (current_time - lock_time).total_seconds()
        return diff > STALE_TIMEOUT_SECONDS
    except (ValueError, TypeError):
        return True  # Can't parse timestamp — treat as stale


def _try_write_lock(lock_file, feature):
    """Atomically create a new lock file.

    Uses O_CREAT | O_EXCL so exactly one process wins when multiple
    race to create the file.  Returns lock_data on success, None on
    FileExistsError (another process created it first).
    """
    try:
        fd = os.open(lock_file, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError:
        return None
    lock_data = {
        "pid": os.getppid(),
        "feature": feature,
        "acquired_at": now(),
    }
    try:
        os.write(fd, json.dumps(lock_data, indent=2).encode())
    finally:
        os.close(fd)
    return lock_data


def _locked_by_winner(lock_file):
    """Re-read after losing a race; return a locked result for whoever won."""
    existing, _ = _read_lock(lock_file)
    if existing is None:
        return {"status": "locked", "feature": "unknown", "pid": 0,
                "acquired_at": "unknown"}
    return {
        "status": "locked",
        "feature": existing["feature"],
        "pid": existing["pid"],
        "acquired_at": existing["acquired_at"],
    }


def _break_and_acquire(lock_file, feature, stale_feature=None):
    """Break a stale/corrupted lock and acquire a new one."""
    lock_file.unlink(missing_ok=True)
    lock_data = _try_write_lock(lock_file, feature)
    if lock_data is None:
        return _locked_by_winner(lock_file)
    result = {"status": "acquired", "stale_broken": True}
    if stale_feature is not None:
        result["stale_feature"] = stale_feature
    return result


def acquire(feature):
    """Attempt to acquire the start lock."""
    lock_file = _lock_path()
    existing, file_existed = _read_lock(lock_file)

    if existing is None:
        if file_existed:
            return _break_and_acquire(lock_file, feature)
        lock_data = _try_write_lock(lock_file, feature)
        if lock_data is not None:
            return {"status": "acquired"}
        return _locked_by_winner(lock_file)

    pid = existing["pid"]
    existing_feature = existing["feature"]
    acquired_at = existing["acquired_at"]

    if _is_timed_out(acquired_at):
        return _break_and_acquire(lock_file, feature, existing_feature)

    return {
        "status": "locked",
        "feature": existing_feature,
        "pid": pid,
        "acquired_at": acquired_at,
    }


def acquire_with_wait(feature, timeout=300, interval=10):
    """Acquire the lock, retrying with sleep until acquired or timed out."""
    start = time.monotonic()
    result = acquire(feature)
    if result["status"] == "acquired":
        return result

    while True:
        elapsed = time.monotonic() - start
        if elapsed >= timeout:
            return {
                "status": "timeout",
                "feature": result["feature"],
                "pid": result["pid"],
                "waited_seconds": int(elapsed),
            }
        remaining = timeout - elapsed
        time.sleep(min(interval, remaining))
        result = acquire(feature)
        if result["status"] == "acquired":
            return result


def release():
    """Release the start lock."""
    lock_file = _lock_path()
    lock_file.unlink(missing_ok=True)
    return {"status": "released"}


def check():
    """Check the current lock status without modifying."""
    lock_file = _lock_path()
    existing, _ = _read_lock(lock_file)

    if existing is None:
        return {"status": "free"}

    if _is_timed_out(existing["acquired_at"]):
        return {"status": "free"}

    return {
        "status": "locked",
        "feature": existing["feature"],
        "pid": existing["pid"],
        "acquired_at": existing["acquired_at"],
    }


def main():
    parser = argparse.ArgumentParser(description="FLOW start lock")
    parser.add_argument("--acquire", action="store_true", help="Acquire the lock")
    parser.add_argument("--release", action="store_true", help="Release the lock")
    parser.add_argument("--check", action="store_true", help="Check lock status")
    parser.add_argument("--feature", default=None, help="Feature name (required for --acquire)")
    parser.add_argument("--wait", action="store_true", help="Wait for lock to be released")
    parser.add_argument("--timeout", type=int, default=300, help="Max seconds to wait (default 300)")
    args = parser.parse_args()

    if args.acquire:
        if not args.feature:
            print(json.dumps({"status": "error", "message": "--feature required for --acquire"}))
            sys.exit(1)
        if args.wait:
            result = acquire_with_wait(args.feature, timeout=args.timeout)
        else:
            result = acquire(args.feature)
    elif args.release:
        result = release()
    elif args.check:
        result = check()
    else:
        print(json.dumps({"status": "error", "message": "Specify --acquire, --release, or --check"}))
        sys.exit(1)

    print(json.dumps(result))


if __name__ == "__main__":
    main()
