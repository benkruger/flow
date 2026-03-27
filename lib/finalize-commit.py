"""Finalize a commit: commit from message file, clean up, pull, push.

Usage:
  bin/flow finalize-commit <message-file> <branch>

Consolidates commit + cleanup + pull + push into a single call
for performance. Called by the flow-commit skill after message
file is written.

Output (JSON to stdout):
  Success:   {"status": "ok", "sha": "<commit-hash>"}
  Warning:   {"status": "ok", "sha": "", "warning": "..."}
  Conflict:  {"status": "conflict", "files": ["file1.py", ...]}
  Error:     {"status": "error", "step": "commit|pull|push", "message": "..."}
"""

import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT, NETWORK_TIMEOUT, parse_conflict_files


def _remove_message_file(message_file):
    """Remove the commit message file, ignoring errors."""
    try:
        os.remove(message_file)
    except OSError:
        pass


def finalize_commit(message_file, branch):
    """Commit, clean up message file, pull, and push.

    Returns a dict with status and details.
    """
    try:
        result = subprocess.run(
            ["git", "commit", "-F", message_file],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        _remove_message_file(message_file)
        return {"status": "error", "step": "commit", "message": f"git commit timed out after {LOCAL_TIMEOUT}s"}

    _remove_message_file(message_file)

    if result.returncode != 0:
        return {"status": "error", "step": "commit", "message": result.stderr.strip()}

    try:
        result = subprocess.run(
            ["git", "pull", "origin", branch],
            capture_output=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {"status": "error", "step": "pull", "message": f"git pull timed out after {NETWORK_TIMEOUT}s"}
    if result.returncode != 0:
        try:
            status = subprocess.run(
                ["git", "status", "--porcelain"],
                capture_output=True,
                text=True,
                timeout=LOCAL_TIMEOUT,
            )
        except subprocess.TimeoutExpired:
            return {"status": "error", "step": "pull", "message": result.stderr.strip()}
        conflict_files = parse_conflict_files(status.stdout)

        if conflict_files:
            return {"status": "conflict", "files": conflict_files}
        return {"status": "error", "step": "pull", "message": result.stderr.strip()}

    try:
        result = subprocess.run(
            ["git", "push"],
            capture_output=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {"status": "error", "step": "push", "message": f"git push timed out after {NETWORK_TIMEOUT}s"}
    if result.returncode != 0:
        return {"status": "error", "step": "push", "message": result.stderr.strip()}

    try:
        sha = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {"status": "ok", "sha": "", "warning": "commit succeeded but SHA retrieval timed out"}
    if sha.returncode != 0:
        return {"status": "ok", "sha": "", "warning": "commit succeeded but SHA retrieval failed"}

    return {"status": "ok", "sha": sha.stdout.strip()}


def main():
    if len(sys.argv) < 3:
        print(
            json.dumps(
                {
                    "status": "error",
                    "step": "args",
                    "message": "Usage: bin/flow finalize-commit <message-file> <branch>",
                }
            )
        )
        sys.exit(1)

    message_file = sys.argv[1]
    branch = sys.argv[2]

    result = finalize_commit(message_file, branch)
    print(json.dumps(result))

    if result["status"] != "ok":
        sys.exit(1)


if __name__ == "__main__":
    main()
