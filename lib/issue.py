"""Create a GitHub issue via gh CLI.

Usage:
  bin/flow issue --title <title> [--repo <repo>] [--label <label>] [--body <body>]

Wraps `gh issue create` so Claude's Bash command is always a clean
one-liner matching `Bash(bin/flow *)` — no heredocs, no long inline
strings, no permission prompt variance.

Output (JSON to stdout):
  Success: {"status": "ok", "url": "<issue_url>"}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import re
import subprocess
import sys


def detect_repo():
    """Auto-detect GitHub repo from git remote origin URL.

    Returns 'owner/repo' string or None if detection fails.
    """
    try:
        result = subprocess.run(
            ["git", "remote", "get-url", "origin"],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            return None
        url = result.stdout.strip()
        if not url:
            return None
        match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$", url)
        if match:
            return match.group(1)
        return None
    except Exception:
        return None


def create_issue(repo, title, label=None, body=None):
    """Run gh issue create and return the issue URL."""
    cmd = ["gh", "issue", "create", "--repo", repo, "--title", title]
    if label:
        cmd.extend(["--label", label])
    if body:
        cmd.extend(["--body", body])

    result = subprocess.run(cmd, capture_output=True, text=True)

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    url = result.stdout.strip()
    return url, None


def main():
    parser = argparse.ArgumentParser(description="Create a GitHub issue")
    parser.add_argument("--repo", default=None, help="Repository (owner/name)")
    parser.add_argument("--title", required=True, help="Issue title")
    parser.add_argument("--label", default=None, help="Issue label")
    parser.add_argument("--body", default=None, help="Issue body")
    args = parser.parse_args()

    repo = args.repo
    if repo is None:
        repo = detect_repo()
        if repo is None:
            print(json.dumps({
                "status": "error",
                "message": "Could not detect repo from git remote. Use --repo owner/name.",
            }))
            sys.exit(1)

    url, error = create_issue(repo, args.title, args.label, args.body)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", "url": url}))


if __name__ == "__main__":
    main()
