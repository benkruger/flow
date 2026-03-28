"""Create a GitHub blocked-by dependency via REST API.

Usage:
  bin/flow link-blocked-by --repo <owner/repo> --blocked-number N --blocking-number N

Resolves both issue numbers to database IDs (required by the REST API),
then creates the blocked-by dependency relationship.

Output (JSON to stdout):
  Success: {"status": "ok", "blocked": N, "blocking": N}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from issue import fetch_database_id as resolve_database_id

_BLOCKED_BY_RE = re.compile(r"^## Blocked by\s*$", re.MULTILINE)
_NEXT_HEADING_RE = re.compile(r"^## ", re.MULTILINE)


def build_blocked_by_section(body, blocking_number):
    """Add a Blocked by reference to an issue body.

    Appends '- #N' under a '## Blocked by' section. Creates the section
    if it doesn't exist. Skips if the reference already exists.

    Returns the updated body string.
    """
    ref = f"- #{blocking_number}"

    if not body:
        return f"## Blocked by\n\n{ref}\n"

    match = _BLOCKED_BY_RE.search(body)
    if not match:
        # No existing section — append at end
        return body.rstrip() + f"\n\n## Blocked by\n\n{ref}\n"

    # Section exists — find its extent (up to the next ## heading or end)
    section_start = match.end()
    next_heading = _NEXT_HEADING_RE.search(body, section_start)
    section_end = next_heading.start() if next_heading else len(body)
    section_content = body[section_start:section_end]

    # Check for duplicate
    if re.search(rf"^- #{blocking_number}\b", section_content, re.MULTILINE):
        return body

    # Insert reference at end of section (before next heading or end)
    insert_point = section_end
    # Trim trailing whitespace in section to add cleanly
    trimmed_end = body[:insert_point].rstrip()
    return trimmed_end + f"\n{ref}\n" + ("\n" + body[insert_point:] if next_heading else "")


def fetch_issue_body(repo, number):
    """Fetch the current body text of a GitHub issue.

    Returns (body, error). body is a string, empty string, or None (if the
    issue body is null). On error, body is None and error is a string.
    """
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/issues/{number}", "--jq", ".body"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "Fetch body timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    raw = result.stdout.strip()
    if raw == "null":
        return None, None
    return raw, None


def update_issue_body(repo, number, body):
    """Update a GitHub issue's body text.

    Returns error string or None on success.
    """
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/issues/{number}", "--method", "PATCH", "-f", f"body={body}"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return "Update body timed out after 30 seconds"

    if result.returncode != 0:
        return result.stderr.strip() or result.stdout.strip() or "Unknown error"

    return None


def link_blocked_by(repo, blocked_number, blocking_number):
    """Create a blocked-by dependency between two issues.

    Returns (result_dict, error). On success result_dict contains:
      blocked: the blocked issue number
      blocking: the blocking issue number
    """
    blocked_id, error = resolve_database_id(repo, blocked_number)
    if error:
        return None, f"Failed to resolve blocked #{blocked_number}: {error}"

    blocking_id, error = resolve_database_id(repo, blocking_number)
    if error:
        return None, f"Failed to resolve blocking #{blocking_number}: {error}"

    try:
        result = subprocess.run(
            [
                "gh",
                "api",
                f"repos/{repo}/issues/{blocked_number}/dependencies/blocked_by",
                "--method",
                "POST",
                "-F",
                f"issue_id={blocking_id}",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return None, "Link creation timed out after 30 seconds"

    if result.returncode != 0:
        error = result.stderr.strip() or result.stdout.strip() or "Unknown error"
        return None, error

    result = {"blocked": blocked_number, "blocking": blocking_number}

    # Best-effort body update — never fail the overall operation
    body, fetch_err = fetch_issue_body(repo, blocked_number)
    if fetch_err:
        result["body_warning"] = f"Could not fetch body: {fetch_err}"
        return result, None

    new_body = build_blocked_by_section(body, blocking_number)
    if new_body == body:
        # Duplicate — reference already exists, skip update
        return result, None

    update_err = update_issue_body(repo, blocked_number, new_body)
    if update_err:
        result["body_warning"] = f"Could not update body: {update_err}"

    return result, None


def main():
    parser = argparse.ArgumentParser(description="Create a GitHub blocked-by dependency")
    parser.add_argument("--repo", required=True, help="Repository (owner/name)")
    parser.add_argument("--blocked-number", required=True, type=int, help="Issue that is blocked")
    parser.add_argument("--blocking-number", required=True, type=int, help="Issue that blocks")
    args = parser.parse_args()

    result, error = link_blocked_by(
        args.repo,
        args.blocked_number,
        args.blocking_number,
    )

    if error:
        print(json.dumps({"status": "error", "message": error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", **result}))


if __name__ == "__main__":
    main()
