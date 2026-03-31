"""Serialize flow-start with a queue directory.

Prevents concurrent starts from fighting over main (CI fixes, dependency
updates). Only one flow-start runs at a time. The oldest queue entry
(by mtime, then feature name) holds the lock.

Usage:
    bin/flow start-lock --acquire --feature <name>
    bin/flow start-lock --acquire --wait --feature <name>
    bin/flow start-lock --acquire --wait --timeout 90 --feature <name>
    bin/flow start-lock --release --feature <name>
    bin/flow start-lock --check

Output (JSON to stdout, all responses include "lock_path"):
    Acquire: {"status": "acquired", "lock_path": ...}
             or {"status": "locked", "feature": ..., "lock_path": ...}
    Acquire --wait: {"status": "acquired", ...}
             or {"status": "timeout", ..., "lock_path": ...}
    Release: {"status": "released", "lock_path": ...}
             or {"status": "error", "message": ..., "lock_path": ...}
    Check:   {"status": "free", "lock_path": ...}
             or {"status": "locked", ..., "lock_path": ...}
"""

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import project_root

QUEUE_DIRNAME = "start-queue"
STALE_TIMEOUT_SECONDS = 1800  # 30 minutes

_CACHED_QUEUE_PATH = None


def _queue_path():
    """Return the path to the queue directory. Cached after first resolution."""
    global _CACHED_QUEUE_PATH
    if _CACHED_QUEUE_PATH is not None:
        return _CACHED_QUEUE_PATH
    root = project_root().resolve()
    state_dir = root / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    queue_dir = state_dir / QUEUE_DIRNAME
    queue_dir.mkdir(exist_ok=True)
    _CACHED_QUEUE_PATH = queue_dir
    return _CACHED_QUEUE_PATH


def _list_queue(queue_dir, cleanup=False):
    """List queue entries sorted by (mtime, name).

    If cleanup=True, remove stale entries (>30 min) before sorting.
    Returns list of (mtime, name) tuples and whether any stale entries
    were removed.
    """
    stale_removed = False
    entries = []

    try:
        items = list(queue_dir.iterdir())
    except OSError:
        return [], False

    for item in items:
        if not item.is_file():
            continue
        try:
            mtime = item.stat().st_mtime
        except OSError:
            continue
        if cleanup and (time.time() - mtime) > STALE_TIMEOUT_SECONDS:
            item.unlink(missing_ok=True)
            stale_removed = True
            continue
        entries.append((mtime, item.name))

    entries.sort()
    return entries, stale_removed


def acquire(feature):
    """Attempt to acquire the start lock via the queue."""
    queue_dir = _queue_path()

    # Create our queue entry (idempotent — overwrites if exists)
    entry = queue_dir / feature
    entry.touch(exist_ok=True)

    # List queue with stale cleanup
    entries, stale_removed = _list_queue(queue_dir, cleanup=True)

    if not entries:
        # Should not happen — we just created our entry
        return {"status": "acquired", "lock_path": str(queue_dir)}

    holder = entries[0][1]
    if holder == feature:
        result = {"status": "acquired", "lock_path": str(queue_dir)}
        if stale_removed:
            result["stale_broken"] = True
        return result

    result = {
        "status": "locked",
        "feature": holder,
        "lock_path": str(queue_dir),
    }
    if stale_removed:
        result["stale_broken"] = True
    return result


def acquire_with_wait(feature, timeout=90, interval=10):
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
                "waited_seconds": int(elapsed),
                "lock_path": result["lock_path"],
            }
        remaining = timeout - elapsed
        time.sleep(min(interval, remaining))
        result = acquire(feature)
        if result["status"] == "acquired":
            return result


def release(feature):
    """Release the start lock by removing our queue entry."""
    queue_dir = _queue_path()
    entry = queue_dir / feature
    entry.unlink(missing_ok=True)
    if entry.exists():
        return {"status": "error", "message": "Queue entry persists after unlink", "lock_path": str(queue_dir)}
    return {"status": "released", "lock_path": str(queue_dir)}


def check():
    """Check the current lock status without modifying."""
    queue_dir = _queue_path()

    # Read-only check — filter stale but don't delete
    entries = []
    try:
        for item in queue_dir.iterdir():
            if not item.is_file():
                continue
            try:
                mtime = item.stat().st_mtime
            except OSError:
                continue
            if (time.time() - mtime) > STALE_TIMEOUT_SECONDS:
                continue
            entries.append((mtime, item.name))
    except OSError:
        pass

    if not entries:
        return {"status": "free", "lock_path": str(queue_dir)}

    entries.sort()
    holder = entries[0][1]
    return {
        "status": "locked",
        "feature": holder,
        "lock_path": str(queue_dir),
    }


def main():
    parser = argparse.ArgumentParser(description="FLOW start lock")
    parser.add_argument("--acquire", action="store_true", help="Acquire the lock")
    parser.add_argument("--release", action="store_true", help="Release the lock")
    parser.add_argument("--check", action="store_true", help="Check lock status")
    parser.add_argument("--feature", default=None, help="Feature name (required for --acquire and --release)")
    parser.add_argument("--wait", action="store_true", help="Wait for lock to be released")
    parser.add_argument("--timeout", type=int, default=90, help="Max seconds to wait (default 90)")
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
        if not args.feature:
            print(json.dumps({"status": "error", "message": "--feature required for --release"}))
            sys.exit(1)
        result = release(args.feature)
    elif args.check:
        result = check()
    else:
        print(json.dumps({"status": "error", "message": "Specify --acquire, --release, or --check"}))
        sys.exit(1)

    print(json.dumps(result))


if __name__ == "__main__":
    main()
