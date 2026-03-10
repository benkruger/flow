"""Create a GitHub issue via gh CLI.

Usage:
  bin/flow issue --repo <repo> --title <title> [--label <label>] [--body <body>]

Wraps `gh issue create` so Claude's Bash command is always a clean
one-liner matching `Bash(bin/flow *)` — no heredocs, no long inline
strings, no permission prompt variance.

Output (JSON to stdout):
  Success: {"status": "ok", "url": "<issue_url>"}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys


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
    parser.add_argument("--repo", required=True, help="Repository (owner/name)")
    parser.add_argument("--title", required=True, help="Issue title")
    parser.add_argument("--label", default=None, help="Issue label")
    parser.add_argument("--body", default=None, help="Issue body")
    args = parser.parse_args()

    url, error = create_issue(args.repo, args.title, args.label, args.body)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", "url": url}))


if __name__ == "__main__":
    main()
