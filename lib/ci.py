"""Run the target project's bin/ci with optional dirty-check optimization.

Usage:
  bin/flow ci [--if-dirty]

Without --if-dirty, always runs bin/ci.
With --if-dirty, skips if nothing changed since the last passing run.

Output (JSON to stdout):
  Success:  {"status": "ok", "skipped": false}
  Skipped:  {"status": "ok", "skipped": true, "reason": "..."}
  Error:    {"status": "error", "message": "..."}
"""

import json
import os
import subprocess
import sys
from pathlib import Path


def _tree_snapshot(root):
    """Return `git status --porcelain` output as a snapshot string."""
    result = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=str(root), capture_output=True, text=True,
    )
    return result.stdout


def main():
    if os.environ.get("FLOW_CI_RUNNING"):
        print(json.dumps({"status": "ok", "skipped": True, "reason": "recursion guard"}))
        sys.exit(0)

    # Set guard immediately so child processes (bin/ci → pytest → bin/flow ci)
    # see it in their inherited environment and short-circuit.
    os.environ["FLOW_CI_RUNNING"] = "1"

    args = sys.argv[1:]
    if_dirty = "--if-dirty" in args

    root = Path.cwd()
    bin_ci = root / "bin" / "ci"
    sentinel = root / ".flow-states" / ".ci-passed"

    if not bin_ci.exists():
        print(json.dumps({"status": "error", "message": "bin/ci not found"}))
        sys.exit(1)

    snapshot = _tree_snapshot(root)

    if if_dirty and sentinel.exists():
        if sentinel.read_text() == snapshot:
            print(json.dumps({"status": "ok", "skipped": True, "reason": "no changes since last CI pass"}))
            sys.exit(0)

    result = subprocess.run(
        ["bash", str(bin_ci)],
        cwd=str(root),
    )

    if result.returncode == 0:
        sentinel.parent.mkdir(parents=True, exist_ok=True)
        sentinel.write_text(snapshot)
        print(json.dumps({"status": "ok", "skipped": False}))
        sys.exit(0)
    else:
        if sentinel.exists():
            sentinel.unlink()
        print(json.dumps({"status": "error", "message": "bin/ci failed"}))
        sys.exit(1)


if __name__ == "__main__":
    main()
