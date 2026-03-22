"""Create a GitHub blocked-by dependency via REST API.

Usage:
  bin/flow link-blocked-by --repo <owner/repo> --blocked-number N --blocking-number N

Resolves both issue numbers to database IDs (required by the REST API),
then creates the blocked-by dependency relationship.

Output (JSON to stdout):
  Success: {"status": "ok", "blocked": N, "blocking": N}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys


def resolve_database_id(repo, issue_number):
    """Resolve an issue number to its REST API database ID.

    Returns (id, error). id is an integer or None.
    """
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/issues/{issue_number}",
             "--jq", ".id"],
            capture_output=True, text=True, timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "gh api timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or "Unknown error"
        return None, error

    try:
        return int(result.stdout.strip()), None
    except (ValueError, TypeError):
        return None, f"Invalid ID from API: {result.stdout.strip()}"


def link_blocked_by(repo, blocked_number, blocking_number):
    """Create a blocked-by dependency between two issues.

    Returns (result_dict, error). On success result_dict contains:
      blocked: the blocked issue number
      blocking: the blocking issue number
    """
    blocked_id, error = resolve_database_id(repo, blocked_number)
    if error:
        return None, f"Failed to resolve blocked #{blocked_number}: {error}"

    blocking_id, error = resolve_database_id(repo, blocking_number)
    if error:
        return None, f"Failed to resolve blocking #{blocking_number}: {error}"

    try:
        result = subprocess.run(
            [
                "gh", "api",
                f"repos/{repo}/issues/{blocked_number}/dependencies/blocked_by",
                "--method", "POST",
                "-f", f"issue_id={blocking_id}",
            ],
            capture_output=True, text=True, timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "Link creation timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    return {"blocked": blocked_number, "blocking": blocking_number}, None


def main():
    parser = argparse.ArgumentParser(
        description="Create a GitHub blocked-by dependency")
    parser.add_argument("--repo", required=True,
                        help="Repository (owner/name)")
    parser.add_argument("--blocked-number", required=True, type=int,
                        help="Issue that is blocked")
    parser.add_argument("--blocking-number", required=True, type=int,
                        help="Issue that blocks")
    args = parser.parse_args()

    result, error = link_blocked_by(
        args.repo, args.blocked_number, args.blocking_number,
    )

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", **result}))


if __name__ == "__main__":
    main()
