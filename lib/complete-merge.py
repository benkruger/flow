"""Consolidated merge for FLOW Complete phase.

Absorbs Step 8: freshness check + squash merge.

Usage: bin/flow complete-merge --pr <number> --state-file <path>

Output (JSON to stdout):
  Merged:     {"status": "merged", "pr_number": N}
  CI rerun:   {"status": "ci_rerun", "pushed": true}
  Conflict:   {"status": "conflict", "conflict_files": [...]}
  CI pending: {"status": "ci_pending"}
  Max retry:  {"status": "max_retries"}
  Error:      {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT, NETWORK_TIMEOUT, mutate_state, project_root

BIN_FLOW = str(Path(__file__).resolve().parent.parent / "bin" / "flow")
MERGE_STEP = 5


def _run_cmd(args, timeout=LOCAL_TIMEOUT):
    """Run a command, returning CompletedProcess. Never raises."""
    try:
        return subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=args,
            returncode=1,
            stdout="",
            stderr=f"Timed out after {timeout}s",
        )


def complete_merge(pr_number, state_file, root=None):
    """Run the freshness check and squash merge.

    Args:
        pr_number: PR number to merge.
        state_file: Path to state file (for check-freshness retries and step counter).
        root: Project root path. Auto-detected if None.

    Returns a result dict with status and details.
    """
    if root is None:
        root = project_root()
    else:
        root = Path(root)

    # Set step counter
    state_path = Path(state_file)
    if state_path.exists():

        def _set_step(s):
            s["complete_step"] = MERGE_STEP

        mutate_state(state_path, _set_step)

    # Run check-freshness
    freshness_result = _run_cmd(
        [BIN_FLOW, "check-freshness", "--state-file", str(state_file)],
        timeout=NETWORK_TIMEOUT,
    )

    # Parse check-freshness output
    try:
        freshness = json.loads(freshness_result.stdout.strip())
    except (json.JSONDecodeError, ValueError):
        return {
            "status": "error",
            "message": f"Invalid JSON from check-freshness: {freshness_result.stdout}",
            "pr_number": pr_number,
        }

    freshness_status = freshness.get("status")

    # Handle max_retries
    if freshness_status == "max_retries":
        return {"status": "max_retries", "pr_number": pr_number}

    # Handle error
    if freshness_status == "error":
        return {
            "status": "error",
            "message": freshness.get("message", "check-freshness failed"),
            "pr_number": pr_number,
        }

    # Handle conflict — map "files" to "conflict_files"
    if freshness_status == "conflict":
        return {
            "status": "conflict",
            "conflict_files": freshness.get("files", []),
            "pr_number": pr_number,
        }

    # Handle merged (main had new commits, merged into branch)
    if freshness_status == "merged":
        push_result = _run_cmd(["git", "push"], timeout=NETWORK_TIMEOUT)
        if push_result.returncode != 0:
            return {
                "status": "error",
                "message": f"Push failed after freshness merge: {push_result.stderr.strip()}",
                "pr_number": pr_number,
            }
        return {"status": "ci_rerun", "pushed": True, "pr_number": pr_number}

    # Handle up_to_date — proceed to squash merge
    if freshness_status == "up_to_date":
        merge_result = _run_cmd(
            ["gh", "pr", "merge", str(pr_number), "--squash"],
            timeout=NETWORK_TIMEOUT,
        )
        if merge_result.returncode == 0:
            return {"status": "merged", "pr_number": pr_number}

        # Check for branch protection error
        stderr = merge_result.stderr.strip()
        if "base branch policy" in stderr:
            return {"status": "ci_pending", "pr_number": pr_number}

        return {"status": "error", "message": stderr, "pr_number": pr_number}

    # Unknown status
    return {
        "status": "error",
        "message": f"Unexpected check-freshness status: {freshness_status}",
        "pr_number": pr_number,
    }


def main():
    parser = argparse.ArgumentParser(description="FLOW Complete phase merge")
    parser.add_argument("--pr", type=int, required=True, help="PR number to merge")
    parser.add_argument("--state-file", required=True, help="Path to state file")
    args = parser.parse_args()

    result = complete_merge(pr_number=args.pr, state_file=args.state_file)
    print(json.dumps(result))

    if result["status"] not in ("merged",):
        sys.exit(1)


if __name__ == "__main__":
    main()
