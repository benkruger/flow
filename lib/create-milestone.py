"""Create a GitHub milestone via gh API.

Usage:
  bin/flow create-milestone --repo <owner/repo> --title <title> --due-date <YYYY-MM-DD>

Output (JSON to stdout):
  Success: {"status": "ok", "number": N, "url": "..."}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys


def create_milestone(repo, title, due_date):
    """Create a milestone via gh api.

    Returns (result_dict, error). On success result_dict contains:
      number: the milestone number
      url: the milestone URL
    """
    try:
        result = subprocess.run(
            [
                "gh",
                "api",
                f"repos/{repo}/milestones",
                "--method",
                "POST",
                "-f",
                f"title={title}",
                "-f",
                f"due_on={due_date}T00:00:00Z",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "Command timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None, f"Invalid JSON response: {result.stdout.strip()}"

    number = data.get("number")
    if number is None:
        return None, "API response missing 'number' field"

    return {
        "number": number,
        "url": data.get("html_url", ""),
    }, None


def main():
    parser = argparse.ArgumentParser(description="Create a GitHub milestone")
    parser.add_argument("--repo", required=True, help="Repository (owner/name)")
    parser.add_argument("--title", required=True, help="Milestone title")
    parser.add_argument("--due-date", required=True, help="Due date (YYYY-MM-DD)")
    args = parser.parse_args()

    result, error = create_milestone(args.repo, args.title, args.due_date)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", **result}))


if __name__ == "__main__":
    main()
