"""Run the target project's bin/dependencies and report changes.

Usage:
  bin/flow update-deps

Checks if bin/dependencies exists in the current directory. If it does,
runs it and checks git status for changes. Returns structured JSON.

Output (JSON to stdout):
  Skipped:  {"status": "skipped", "reason": "bin/dependencies not found"}
  No changes: {"status": "ok", "changes": false}
  Changes:  {"status": "ok", "changes": true}
  Error:    {"status": "error", "message": "..."}

Environment:
  FLOW_UPDATE_DEPS_TIMEOUT — timeout in seconds (default: 300)
"""

import json
import os
import signal
import subprocess
import sys
from pathlib import Path


def main():
    cwd = Path.cwd()
    deps = cwd / "bin" / "dependencies"

    if not deps.exists():
        print(json.dumps({"status": "skipped", "reason": "bin/dependencies not found"}))
        sys.exit(0)

    timeout = int(os.environ.get("FLOW_UPDATE_DEPS_TIMEOUT", "300"))

    try:
        proc = subprocess.Popen(
            [str(deps)],
            cwd=str(cwd),
            start_new_session=True,
        )
        proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        # Kill the entire process group so child processes (e.g. sleep)
        # don't survive as orphans.
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
        print(json.dumps({"status": "error", "message": f"bin/dependencies timed out after {timeout}s"}))
        sys.exit(1)

    result = proc

    if result.returncode != 0:
        print(json.dumps({"status": "error", "message": f"bin/dependencies failed with exit code {result.returncode}"}))
        sys.exit(1)

    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=str(cwd),
        capture_output=True,
        text=True,
    )

    changes = bool(status.stdout.strip())
    print(json.dumps({"status": "ok", "changes": changes}))
    sys.exit(0)


if __name__ == "__main__":
    main()
