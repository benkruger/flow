"""Pre-merge freshness check: fetch main, verify branch is up-to-date.

Usage:
  bin/flow check-freshness [--state-file <path>]

Checks whether the feature branch contains the latest origin/main.
If not, merges origin/main into the current branch.

Output (JSON to stdout):
  Up-to-date:   {"status": "up_to_date"}
  Merged:       {"status": "merged"} or {"status": "merged", "retries": N}
  Conflict:     {"status": "conflict", "files": ["..."]} or with "retries"
  Max retries:  {"status": "max_retries", "retries": N}
  Error:        {"status": "error", "step": "fetch|merge", "message": "..."}

When --state-file is provided, tracks freshness_retries in the state file
and stops at 3 retries (configurable via MAX_RETRIES).
"""

import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT, NETWORK_TIMEOUT, mutate_state, parse_conflict_files

MAX_RETRIES = 3


def _check_and_increment_retries(state_file, increment=False):
    """Check retry count and optionally increment, atomically.

    When increment=False, reads the current count without modifying.
    When increment=True, increments and returns the new count.
    Both operations happen under the mutate_state lock to prevent races.
    """
    count = 0

    def _transform(state):
        nonlocal count
        current = state.get("freshness_retries", 0)
        if increment:
            count = current + 1
            state["freshness_retries"] = count
        else:
            count = current

    mutate_state(state_file, _transform)
    return count


def check_freshness(state_file=None):
    """Check if branch is up-to-date with origin/main.

    Returns a dict with status and details.
    """
    # Check retry limit
    if state_file:
        retries = _check_and_increment_retries(state_file, increment=False)
        if retries >= MAX_RETRIES:
            return {"status": "max_retries", "retries": retries}

    # Step 1: fetch origin main
    try:
        result = subprocess.run(
            ["git", "fetch", "origin", "main"],
            capture_output=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "error",
            "step": "fetch",
            "message": f"git fetch timed out after {NETWORK_TIMEOUT}s",
        }

    if result.returncode != 0:
        return {
            "status": "error",
            "step": "fetch",
            "message": result.stderr.strip(),
        }

    # Step 2: check if origin/main is already an ancestor of HEAD
    try:
        result = subprocess.run(
            ["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
        if result.returncode == 0:
            return {"status": "up_to_date"}
    except subprocess.TimeoutExpired:
        # Treat timeout as "not sure" — proceed to merge attempt
        pass

    # Step 3: merge origin/main
    try:
        result = subprocess.run(
            ["git", "merge", "origin/main"],
            capture_output=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "error",
            "step": "merge",
            "message": f"git merge timed out after {NETWORK_TIMEOUT}s",
        }

    if result.returncode == 0:
        # Merge succeeded — increment retries if tracking
        response = {"status": "merged"}
        if state_file:
            response["retries"] = _check_and_increment_retries(
                state_file,
                increment=True,
            )
        return response

    # Merge failed — check for conflicts
    try:
        status = subprocess.run(
            ["git", "status", "--porcelain"],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "error",
            "step": "merge",
            "message": result.stderr.strip(),
        }

    conflict_files = parse_conflict_files(status.stdout)

    if conflict_files:
        response = {"status": "conflict", "files": conflict_files}
        if state_file:
            response["retries"] = _check_and_increment_retries(
                state_file,
                increment=True,
            )
        return response

    return {
        "status": "error",
        "step": "merge",
        "message": result.stderr.strip(),
    }


def main():
    state_file = None
    args = sys.argv[1:]

    i = 0
    while i < len(args):
        if args[i] == "--state-file" and i + 1 < len(args):
            state_file = args[i + 1]
            i += 2
        else:
            i += 1

    result = check_freshness(state_file=state_file)
    print(json.dumps(result))

    if result["status"] not in ("up_to_date", "merged"):
        sys.exit(1)


if __name__ == "__main__":
    main()
