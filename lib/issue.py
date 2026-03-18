"""Create a GitHub issue via gh CLI.

Usage:
  bin/flow issue --title <title> [--repo <repo>] [--label <label>] [--body-file <path>]

Body text is always passed via a file to avoid shell escaping issues
with special characters (|, &&, ;) that trigger the Bash hook validator.
The file is read and deleted before the gh call.

Output (JSON to stdout):
  Success: {"status": "ok", "url": "<issue_url>"}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import detect_repo


def read_body_file(path):
    """Read body text from a file and delete the file.

    Returns (body_text, error_message). On success error is None.
    The file is always deleted after reading, even if empty.
    """
    try:
        body = open(path).read()
    except (OSError, IOError) as exc:
        return None, f"Could not read body file '{path}': {exc}"

    try:
        os.remove(path)
    except OSError:
        pass

    return body, None


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
    parser.add_argument("--body-file", default=None,
                        help="Path to file containing issue body (file is deleted after reading)")
    parser.add_argument("--state-file", default=None,
                        help="Path to state file for repo lookup (checks state before detect_repo)")
    args = parser.parse_args()

    repo = args.repo
    if repo is None and args.state_file:
        try:
            from pathlib import Path as _Path
            state = json.loads(_Path(args.state_file).read_text())
            repo = state.get("repo")
        except (OSError, json.JSONDecodeError):
            pass
    if repo is None:
        repo = detect_repo()
        if repo is None:
            print(json.dumps({
                "status": "error",
                "message": "Could not detect repo from git remote. Use --repo owner/name.",
            }))
            sys.exit(1)

    body = None
    if args.body_file:
        body, read_error = read_body_file(args.body_file)
        if read_error:
            print(json.dumps({"status": "error", "message": read_error}))
            sys.exit(1)

    url, error = create_issue(repo, args.title, args.label, body)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", "url": url}))


if __name__ == "__main__":
    main()
