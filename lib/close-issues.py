"""Close GitHub issues referenced in the FLOW start prompt.

Reads the state file, extracts #N patterns from the prompt field,
and closes each issue via gh CLI after the PR is merged.

Usage: bin/flow close-issues --state-file <path>

Output (JSON to stdout):
  {"status": "ok", "closed": [83, 89], "failed": []}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import extract_issue_numbers


def close_issues(issue_numbers, repo=None):
    """Close each issue via gh CLI. Returns dict with closed and failed lists.

    When repo is provided (e.g. "owner/repo"), closed items include URLs.
    """
    closed = []
    failed = []
    for num in issue_numbers:
        try:
            result = subprocess.run(
                ["gh", "issue", "close", str(num)],
                capture_output=True,
                text=True,
                timeout=30,
            )
            if result.returncode == 0:
                entry = {"number": num}
                if repo:
                    entry["url"] = f"https://github.com/{repo}/issues/{num}"
                closed.append(entry)
            else:
                failed.append(num)
        except subprocess.TimeoutExpired:
            failed.append(num)
    return {"closed": closed, "failed": failed}


def main():
    parser = argparse.ArgumentParser(description="Close issues from FLOW prompt")
    parser.add_argument("--state-file", required=True, help="Path to state JSON file")
    args = parser.parse_args()

    try:
        state = json.loads(Path(args.state_file).read_text())
    except Exception as exc:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Could not read state file: {exc}",
                }
            )
        )
        sys.exit(1)
    prompt = state.get("prompt", "")
    repo = state.get("repo")
    issue_numbers = extract_issue_numbers(prompt)
    result = close_issues(issue_numbers, repo=repo)

    output = {"status": "ok", **result}
    print(json.dumps(output))


if __name__ == "__main__":
    main()
