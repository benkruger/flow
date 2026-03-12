"""Close a single GitHub issue via gh CLI.

Usage:
  bin/flow close-issue --number <N> [--repo <repo>]

Wraps `gh issue close` so Claude's Bash command is always a clean
one-liner matching `Bash(bin/flow *)` — no heredocs, no long inline
strings, no permission prompt variance.

Output (JSON to stdout):
  Success: {"status": "ok"}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import re
import subprocess
import sys


def detect_repo_or_fail():
    """Auto-detect GitHub repo from git remote origin URL.

    Returns 'owner/repo' string or exits with error JSON.
    """
    error_message = "Could not detect repo from git remote. Use --repo owner/name."
    try:
        result = subprocess.run(
            ["git", "remote", "get-url", "origin"],
            capture_output=True, text=True,
        )
        if result.returncode == 0:
            url = result.stdout.strip()
            if url:
                match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$", url)
                if match:
                    return match.group(1)
    except Exception:
        pass
    print(json.dumps({"status": "error", "message": error_message}))
    sys.exit(1)


def close_issue_by_number(repo, number):
    """Close a GitHub issue and return error message or None on success."""
    result = subprocess.run(
        ["gh", "issue", "close", "--repo", repo, str(number)],
        capture_output=True, text=True,
    )

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return error

    return None


def main():
    parser = argparse.ArgumentParser(description="Close a GitHub issue")
    parser.add_argument("--repo", default=None, help="Repository (owner/name)")
    parser.add_argument("--number", required=True, type=int, help="Issue number")
    args = parser.parse_args()

    repo = args.repo
    if repo is None:
        repo = detect_repo_or_fail()

    error = close_issue_by_number(repo, args.number)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok"}))


if __name__ == "__main__":
    main()
