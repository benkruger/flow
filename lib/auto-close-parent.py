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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT


def _run_api(cmd):
    """Run a gh command, returning (CompletedProcess, error_str).

    Returns (result, None) on success, (None, error) on failure or timeout.
    """
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=LOCAL_TIMEOUT)
    except subprocess.TimeoutExpired:
        return None, "timed out"
    if result.returncode != 0:
        return None, result.stderr.strip() or "Unknown error"
    return result, None


def _fetch_issue_fields(repo, issue_number):
    """Fetch parent_issue.number and milestone.number in one API call.

    Returns (parent_number_or_None, milestone_number_or_None).
    Best-effort: returns (None, None) on any failure.
    """
    result, error = _run_api([
        "gh", "api", f"repos/{repo}/issues/{issue_number}",
    ])
    if error:
        return None, None

    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None, None

    parent_number = None
    parent_issue = data.get("parent_issue")
    if isinstance(parent_issue, dict):
        raw = parent_issue.get("number")
        if isinstance(raw, int):
            parent_number = raw

    milestone_number = None
    milestone = data.get("milestone")
    if isinstance(milestone, dict):
        raw = milestone.get("number")
        if isinstance(raw, int):
            milestone_number = raw

    return parent_number, milestone_number


def check_parent_closed(repo, issue_number, parent_number=None):
    """Check if all sub-issues of the parent are closed; close parent if so.

    If parent_number is provided, uses it directly (skips the lookup).
    Returns True if the parent was closed, False otherwise.
    Best-effort: any failure returns False.
    """
    if parent_number is None:
        # Standalone call — fetch the parent number
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


def check_milestone_closed(repo, issue_number, milestone_number=None):
    """Check if all milestone issues are closed; close milestone if so.

    If milestone_number is provided, uses it directly (skips the lookup).
    Returns True if the milestone was closed, False otherwise.
    Best-effort: any failure returns False.
    """
    if milestone_number is None:
        # Standalone call — fetch the milestone number
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

    # Check milestone open_issues count — default to 1 so a missing
    # field is treated as open, never accidentally closing
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

    # Fetch both fields in one API call to avoid redundant requests
    parent_number, milestone_number = _fetch_issue_fields(
        args.repo, args.issue_number,
    )

    parent_closed = check_parent_closed(
        args.repo, args.issue_number, parent_number=parent_number,
    )
    milestone_closed = check_milestone_closed(
        args.repo, args.issue_number, milestone_number=milestone_number,
    )

    print(json.dumps({
        "status": "ok",
        "parent_closed": parent_closed,
        "milestone_closed": milestone_closed,
    }))


if __name__ == "__main__":
    main()
