"""Serialize flow-start with file locking.

Prevents concurrent starts from fighting over main (CI fixes, dependency
updates). Only one flow-start runs at a time. Second start waits or
reports the lock.

Usage:
    bin/flow start-lock --acquire --feature <name>
    bin/flow start-lock --release
    bin/flow start-lock --check

Output (JSON to stdout):
    Acquire: {"status": "acquired"} or {"status": "locked", "feature": ..., "pid": ..., "acquired_at": ...}
    Release: {"status": "released"}
    Check:   {"status": "free"} or {"status": "locked", ...}
"""

import argparse
import json
import os
import sys
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
    """Read and parse lock file. Returns None if missing, empty, or corrupted."""
    if not lock_file.exists():
        return None
    try:
        text = lock_file.read_text().strip()
        if not text:
            return None
        data = json.loads(text)
        if "pid" not in data or "feature" not in data or "acquired_at" not in data:
            return None
        return data
    except (json.JSONDecodeError, OSError):
        return None


def _is_pid_alive(pid):
    """Check if a process is still running."""
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True  # Process exists but we can't signal it


def _is_timed_out(acquired_at):
    """Check if the lock has exceeded the stale timeout."""
    try:
        lock_time = datetime.fromisoformat(acquired_at)
        current_time = datetime.fromisoformat(now())
        diff = (current_time - lock_time).total_seconds()
        return diff > STALE_TIMEOUT_SECONDS
    except (ValueError, TypeError):
        return True  # Can't parse timestamp — treat as stale


def _write_lock(lock_file, feature):
    """Write a new lock file."""
    lock_data = {
        "pid": os.getppid(),
        "feature": feature,
        "acquired_at": now(),
    }
    lock_file.write_text(json.dumps(lock_data, indent=2))
    return lock_data


def acquire(feature):
    """Attempt to acquire the start lock."""
    lock_file = _lock_path()
    existing = _read_lock(lock_file)

    if existing is None:
        # No lock or corrupted — check if file existed (for stale_broken)
        was_stale = lock_file.exists()
        lock_data = _write_lock(lock_file, feature)
        result = {"status": "acquired"}
        if was_stale:
            result["stale_broken"] = True
        return result

    pid = existing["pid"]
    existing_feature = existing["feature"]
    acquired_at = existing["acquired_at"]

    if not _is_pid_alive(pid) or _is_timed_out(acquired_at):
        _write_lock(lock_file, feature)
        return {
            "status": "acquired",
            "stale_broken": True,
            "stale_feature": existing_feature,
        }

    return {
        "status": "locked",
        "feature": existing_feature,
        "pid": pid,
        "acquired_at": acquired_at,
    }


def release():
    """Release the start lock."""
    lock_file = _lock_path()
    lock_file.unlink(missing_ok=True)
    return {"status": "released"}


def check():
    """Check the current lock status without modifying."""
    lock_file = _lock_path()
    existing = _read_lock(lock_file)

    if existing is None:
        return {"status": "free"}

    pid = existing["pid"]
    if not _is_pid_alive(pid):
        return {"status": "free"}

    return {
        "status": "locked",
        "feature": existing["feature"],
        "pid": pid,
        "acquired_at": existing["acquired_at"],
    }


def main():
    parser = argparse.ArgumentParser(description="FLOW start lock")
    parser.add_argument("--acquire", action="store_true", help="Acquire the lock")
    parser.add_argument("--release", action="store_true", help="Release the lock")
    parser.add_argument("--check", action="store_true", help="Check lock status")
    parser.add_argument("--feature", default=None, help="Feature name (required for --acquire)")
    args = parser.parse_args()

    if args.acquire:
        if not args.feature:
            print(json.dumps({"status": "error", "message": "--feature required for --acquire"}))
            sys.exit(1)
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
