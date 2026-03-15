"""Update an issue's status in the FLOW sweep state file.

Usage:
  bin/flow sweep-update --issue <number> --status <status> [options]

Options:
  --pr-url <url>        Set the PR URL
  --pr-number <number>  Set the PR number
  --branch <branch>     Set the branch name
  --worktree <path>     Set the worktree path
  --error <message>     Set the error message (for failed status)

Output (JSON to stdout):
  Success:  {"status": "ok", "issue": <number>, "new_status": "<status>"}
  No sweep: {"status": "no_sweep"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, project_root


def update_issue(sweep_path, issue_number, new_status,
                 pr_url=None, pr_number=None, branch=None,
                 worktree=None, error=None):
    """Update a specific issue in the sweep state file.

    Returns the updated sweep dict.
    """
    sweep = json.loads(sweep_path.read_text())
    issues = sweep.get("issues", [])

    found = False
    for issue in issues:
        if issue.get("number") == issue_number:
            issue["status"] = new_status
            if pr_url is not None:
                issue["pr_url"] = pr_url
            if pr_number is not None:
                issue["pr_number"] = pr_number
            if branch is not None:
                issue["branch"] = branch
            if worktree is not None:
                issue["worktree"] = worktree
            if error is not None:
                issue["error"] = error
            if new_status in ("complete", "failed"):
                issue["completed_at"] = now()
            if new_status == "in_progress" and not issue.get("started_at"):
                issue["started_at"] = now()
            found = True
            break

    if not found:
        return None

    # Check if all issues are done (complete or failed)
    all_done = all(
        i.get("status") in ("complete", "failed")
        for i in issues
    )
    if all_done:
        sweep["status"] = "complete"

    sweep_path.write_text(json.dumps(sweep, indent=2))
    return sweep


def main():
    parser = argparse.ArgumentParser(description="Update sweep issue status")
    parser.add_argument("--issue", type=int, required=True,
                        help="Issue number to update")
    parser.add_argument("--status", required=True,
                        choices=["queued", "in_progress", "complete", "failed"],
                        help="New status")
    parser.add_argument("--pr-url", default=None, help="PR URL")
    parser.add_argument("--pr-number", type=int, default=None, help="PR number")
    parser.add_argument("--branch", default=None, help="Branch name")
    parser.add_argument("--worktree", default=None, help="Worktree path")
    parser.add_argument("--error", default=None, help="Error message")
    args = parser.parse_args()

    root = project_root()
    sweep_path = root / ".flow-states" / "sweep.json"

    if not sweep_path.exists():
        print(json.dumps({"status": "no_sweep"}))
        sys.exit(1)

    try:
        json.loads(sweep_path.read_text())
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read sweep.json: {e}",
        }))
        sys.exit(1)

    try:
        result = update_issue(
            sweep_path, args.issue, args.status,
            pr_url=args.pr_url, pr_number=args.pr_number,
            branch=args.branch, worktree=args.worktree,
            error=args.error,
        )
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Failed to update: {e}",
        }))
        sys.exit(1)

    if result is None:
        print(json.dumps({
            "status": "error",
            "message": f"Issue #{args.issue} not found in sweep.json",
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "issue": args.issue,
        "new_status": args.status,
    }))


if __name__ == "__main__":
    main()
