"""Create a GitHub sub-issue relationship via REST API.

Usage:
  bin/flow create-sub-issue --repo <owner/repo> --parent-number N --child-number N

Resolves both issue numbers to database IDs (required by the REST API),
then creates the sub-issue relationship.

Output (JSON to stdout):
  Success: {"status": "ok", "parent": N, "child": N}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from issue import fetch_database_id as resolve_database_id


def create_sub_issue(repo, parent_number, child_number):
    """Create a sub-issue relationship between two issues.

    Returns (result_dict, error). On success result_dict contains:
      parent: the parent issue number
      child: the child issue number
    """
    parent_id, error = resolve_database_id(repo, parent_number)
    if error:
        return None, f"Failed to resolve parent #{parent_number}: {error}"

    child_id, error = resolve_database_id(repo, child_number)
    if error:
        return None, f"Failed to resolve child #{child_number}: {error}"

    try:
        result = subprocess.run(
            [
                "gh", "api",
                f"repos/{repo}/issues/{parent_number}/sub_issues",
                "--method", "POST",
                "-f", f"sub_issue_id={child_id}",
            ],
            capture_output=True, text=True, timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "Link creation timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    return {"parent": parent_number, "child": child_number}, None


def main():
    parser = argparse.ArgumentParser(
        description="Create a GitHub sub-issue relationship")
    parser.add_argument("--repo", required=True,
                        help="Repository (owner/name)")
    parser.add_argument("--parent-number", required=True, type=int,
                        help="Parent issue number")
    parser.add_argument("--child-number", required=True, type=int,
                        help="Child issue number")
    args = parser.parse_args()

    result, error = create_sub_issue(
        args.repo, args.parent_number, args.child_number,
    )

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", **result}))


if __name__ == "__main__":
    main()
