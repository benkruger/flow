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

sys.path.insert(0, str(Path(__file__).resolve().parent))
from flow_utils import current_branch, project_root


def _tree_snapshot(root):
    """Return HEAD hash + git status as a snapshot string.

    Combines `git rev-parse HEAD` and `git status --porcelain` so the
    snapshot changes after a commit (HEAD moves) even if the working tree
    is clean in both cases.
    """
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=str(root), capture_output=True, text=True,
    )
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=str(root), capture_output=True, text=True,
    )
    return head.stdout.strip() + "\n" + status.stdout


def main():
    if os.environ.get("FLOW_CI_RUNNING"):
        print(json.dumps({"status": "ok", "skipped": True, "reason": "recursion guard"}))
        sys.exit(0)

    # Set guard immediately so child processes (bin/ci → pytest → bin/flow ci)
    # see it in their inherited environment and short-circuit.
    os.environ["FLOW_CI_RUNNING"] = "1"

    args = sys.argv[1:]
    if_dirty = "--if-dirty" in args

    cwd = Path.cwd()
    bin_ci = cwd / "bin" / "ci"
    root = project_root()
    branch = current_branch()
    sentinel = (
        root / ".flow-states" / f"{branch}-ci-passed"
        if branch else None
    )

    if not bin_ci.exists():
        print(json.dumps({"status": "error", "message": "bin/ci not found"}))
        sys.exit(1)

    snapshot = _tree_snapshot(cwd)

    if if_dirty and sentinel and sentinel.exists():
        if sentinel.read_text() == snapshot:
            print(json.dumps({"status": "ok", "skipped": True, "reason": "no changes since last CI pass"}))
            sys.exit(0)

    result = subprocess.run(
        ["bash", str(bin_ci)],
        cwd=str(cwd),
    )

    if result.returncode == 0:
        if sentinel:
            sentinel.parent.mkdir(parents=True, exist_ok=True)
            sentinel.write_text(snapshot)
        print(json.dumps({"status": "ok", "skipped": False}))
        sys.exit(0)
    else:
        if sentinel and sentinel.exists():
            sentinel.unlink()
        print(json.dumps({"status": "error", "message": "bin/ci failed"}))
        sys.exit(1)


if __name__ == "__main__":
    main()
