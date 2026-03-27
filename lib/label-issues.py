"""Add or remove the Flow In-Progress label on GitHub issues.

Reads the state file, extracts #N patterns from the prompt field,
and adds or removes the "Flow In-Progress" label via gh CLI.

Usage:
  bin/flow label-issues --state-file <path> --add
  bin/flow label-issues --state-file <path> --remove

Output (JSON to stdout):
  {"status": "ok", "labeled": [83, 89], "failed": []}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import extract_issue_numbers

LABEL = "Flow In-Progress"


def label_issues(issue_numbers, action="add"):
    """Add or remove label on each issue via gh CLI.

    Returns dict with labeled and failed lists.
    """
    labeled = []
    failed = []
    flag = "--add-label" if action == "add" else "--remove-label"
    for num in issue_numbers:
        try:
            result = subprocess.run(
                ["gh", "issue", "edit", str(num), flag, LABEL],
                capture_output=True,
                text=True,
                timeout=30,
            )
            if result.returncode == 0:
                labeled.append(num)
            else:
                failed.append(num)
        except subprocess.TimeoutExpired:
            failed.append(num)
    return {"labeled": labeled, "failed": failed}


def main():
    parser = argparse.ArgumentParser(description="Label issues from FLOW prompt")
    parser.add_argument("--state-file", required=True, help="Path to state JSON file")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--add", action="store_true", help="Add label")
    group.add_argument("--remove", action="store_true", help="Remove label")
    args = parser.parse_args()

    state = json.loads(Path(args.state_file).read_text())
    prompt = state.get("prompt", "")
    issue_numbers = extract_issue_numbers(prompt)
    action = "add" if args.add else "remove"
    result = label_issues(issue_numbers, action=action)

    output = {"status": "ok", **result}
    print(json.dumps(output))


if __name__ == "__main__":
    main()
