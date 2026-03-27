"""Create a GitHub issue via gh CLI.

Usage:
  bin/flow issue --title <title> [--repo <repo>] [--label <label>] [--body-file <path>]

Body text is always passed via a file to avoid shell escaping issues
with special characters (|, &&, ;) that trigger the Bash hook validator.
The file is read and deleted before the gh call.

Output (JSON to stdout):
  Success: {"status": "ok", "url": "<issue_url>", "number": N, "id": N}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT, detect_repo, project_root


def read_body_file(path):
    """Read body text from a file and delete the file.

    Returns (body_text, error_message). On success error is None.
    The file is always deleted after reading, even if empty.
    """
    if not os.path.isabs(path):
        path = str(project_root() / path)

    try:
        body = open(path).read()
    except (OSError, IOError) as exc:
        return None, f"Could not read body file '{path}': {exc}"

    try:
        os.remove(path)
    except OSError:
        pass

    return body, None


def parse_issue_number(url):
    """Extract issue number from a GitHub issue URL.

    Returns the integer issue number, or None if the URL doesn't match.
    """
    match = re.search(r"/issues/(\d+)", url)
    return int(match.group(1)) if match else None


def fetch_database_id(repo, number):
    """Fetch the REST API database ID for an issue.

    The database ID is the integer ID used by REST API endpoints for
    sub-issues and dependencies. This is NOT the GraphQL node_id.

    Returns (id, error). id is an integer or None.
    """
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/issues/{number}", "--jq", ".id"],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return None, f"gh api timed out after {LOCAL_TIMEOUT}s"

    if result.returncode != 0:
        error = result.stderr.strip() or "Unknown error"
        return None, error

    try:
        return int(result.stdout.strip()), None
    except (ValueError, TypeError):
        return None, f"Invalid ID from API: {result.stdout.strip()}"


def create_issue(repo, title, label=None, body=None):
    """Run gh issue create and return issue details.

    Returns (result_dict, error). On success result_dict contains:
      url: the issue URL
      number: the issue number (int or None)
      id: the REST API database ID (int or None, non-blocking)
    """
    cmd = ["gh", "issue", "create", "--repo", repo, "--title", title]
    if label:
        cmd.extend(["--label", label])
    if body:
        cmd.extend(["--body", body])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=LOCAL_TIMEOUT)
    except subprocess.TimeoutExpired:
        return None, f"Command timed out after {LOCAL_TIMEOUT}s"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"

        # Label-not-found: try creating the label, then retry
        if label and "label" in error.lower() and "not found" in error.lower():
            label_created = False
            try:
                label_result = subprocess.run(
                    ["gh", "label", "create", label, "--repo", repo],
                    capture_output=True,
                    text=True,
                    timeout=LOCAL_TIMEOUT,
                )
                label_created = label_result.returncode == 0
            except subprocess.TimeoutExpired:
                pass

            if label_created:
                retry_cmd = cmd
            else:
                # Label creation failed — retry without label
                retry_cmd = ["gh", "issue", "create", "--repo", repo, "--title", title]
                if body:
                    retry_cmd.extend(["--body", body])

            try:
                retry = subprocess.run(retry_cmd, capture_output=True, text=True, timeout=LOCAL_TIMEOUT)
            except subprocess.TimeoutExpired:
                return None, f"Command timed out after {LOCAL_TIMEOUT}s"

            if retry.returncode == 0:
                result = retry
            else:
                retry_err = retry.stderr.strip() or retry.stdout.strip() or "Unknown error"
                return None, retry_err

        else:
            return None, error

    url = result.stdout.strip()
    number = parse_issue_number(url)
    db_id = None
    if number is not None:
        db_id, _ = fetch_database_id(repo, number)

    return {"url": url, "number": number, "id": db_id}, None


def main():
    parser = argparse.ArgumentParser(description="Create a GitHub issue")
    parser.add_argument("--repo", default=None, help="Repository (owner/name)")
    parser.add_argument("--title", required=True, help="Issue title")
    parser.add_argument("--label", default=None, help="Issue label")
    parser.add_argument(
        "--body-file", default=None, help="Path to file containing issue body (file is deleted after reading)"
    )
    parser.add_argument(
        "--state-file", default=None, help="Path to state file for repo lookup (checks state before detect_repo)"
    )
    args = parser.parse_args()

    repo = args.repo
    if repo is None and args.state_file:
        try:
            state = json.loads(Path(args.state_file).read_text())
            repo = state.get("repo")
        except (OSError, json.JSONDecodeError):
            pass
    if repo is None:
        repo = detect_repo()
        if repo is None:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": "Could not detect repo from git remote. Use --repo owner/name.",
                    }
                )
            )
            sys.exit(1)

    body = None
    if args.body_file:
        body, read_error = read_body_file(args.body_file)
        if read_error:
            print(json.dumps({"status": "error", "message": read_error}))
            sys.exit(1)

    result, error = create_issue(repo, args.title, args.label, body)

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(
        json.dumps(
            {
                "status": "ok",
                "url": result["url"],
                "number": result["number"],
                "id": result["id"],
            }
        )
    )


if __name__ == "__main__":
    main()
