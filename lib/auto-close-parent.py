"""Auto-close parent issue and milestone when all children are done.

Usage:
  bin/flow auto-close-parent --repo <owner/repo> --issue-number N

Checks if the issue has a parent (sub-issue relationship). If so, checks
whether all sibling sub-issues are closed. If all closed, closes the parent.
Also checks the issue's milestone — if all milestone issues are closed,
closes the milestone.

Best-effort throughout — any failure continues silently.

Output (JSON to stdout):
  {"status": "ok", "parent_closed": bool, "milestone_closed": bool}
"""

import argparse
import json
import subprocess
import sys


def _run_api(cmd):
    """Run a gh command, returning (CompletedProcess, error_str).

    Returns (result, None) on success, (None, error) on failure or timeout.
    """
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    except subprocess.TimeoutExpired:
        return None, "timed out"
    if result.returncode != 0:
        return None, result.stderr.strip() or "Unknown error"
    return result, None


def check_parent_closed(repo, issue_number):
    """Check if all sub-issues of the parent are closed; close parent if so.

    Returns True if the parent was closed, False otherwise.
    Best-effort: any failure returns False.
    """
    # Get the parent issue number
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/issues/{issue_number}",
        "--jq", ".parent_issue.number",
    ])
    if error:
        return False

    parent_str = result.stdout.strip()
    if not parent_str or parent_str == "null":
        return False

    try:
        parent_number = int(parent_str)
    except (ValueError, TypeError):
        return False

    # Get all sub-issues of the parent
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/issues/{parent_number}/sub_issues",
    ])
    if error:
        return False

    try:
        sub_issues = json.loads(result.stdout)
    except json.JSONDecodeError:
        return False

    # Check if all sub-issues are closed
    if not sub_issues:
        return False
    if any(si.get("state") != "closed" for si in sub_issues):
        return False

    # All closed — close the parent
    result, error = _run_api([
        "gh", "issue", "close", str(parent_number), "--repo", repo,
    ])
    return error is None


def check_milestone_closed(repo, issue_number):
    """Check if all milestone issues are closed; close milestone if so.

    Returns True if the milestone was closed, False otherwise.
    Best-effort: any failure returns False.
    """
    # Get the milestone number
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/issues/{issue_number}",
        "--jq", ".milestone.number",
    ])
    if error:
        return False

    milestone_str = result.stdout.strip()
    if not milestone_str or milestone_str == "null":
        return False

    try:
        milestone_number = int(milestone_str)
    except (ValueError, TypeError):
        return False

    # Check milestone open_issues count
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/milestones/{milestone_number}",
    ])
    if error:
        return False

    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return False

    if data.get("open_issues", 1) > 0:
        return False

    # All closed — close the milestone
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/milestones/{milestone_number}",
        "--method", "PATCH", "-f", "state=closed",
    ])
    return error is None


def main():
    parser = argparse.ArgumentParser(
        description="Auto-close parent issue and milestone")
    parser.add_argument("--repo", required=True,
                        help="Repository (owner/name)")
    parser.add_argument("--issue-number", required=True, type=int,
                        help="Issue number to check")
    args = parser.parse_args()

    parent_closed = check_parent_closed(args.repo, args.issue_number)
    milestone_closed = check_milestone_closed(args.repo, args.issue_number)

    print(json.dumps({
        "status": "ok",
        "parent_closed": parent_closed,
        "milestone_closed": milestone_closed,
    }))


if __name__ == "__main__":
    main()
