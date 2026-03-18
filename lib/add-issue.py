"""Record a filed GitHub issue in the FLOW state file.

Usage:
  bin/flow add-issue --label <label> --title <title> --url <url> --phase <phase>

Appends to the issues_filed array in the state file. Follows the same
pattern as append-note.py for state file discovery and mutation.

Output (JSON to stdout):
  Success:  {"status": "ok", "issue_count": N}
  No state: {"status": "no_state"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, now, project_root, resolve_branch, PHASE_NAMES


def add_issue(state_path, label, title, url, phase):
    """Append an issue to the state file. Returns the updated state dict."""
    def transform(state):
        if "issues_filed" not in state:
            state["issues_filed"] = []
        state["issues_filed"].append({
            "label": label,
            "title": title,
            "url": url,
            "phase": phase,
            "phase_name": PHASE_NAMES.get(phase, phase),
            "timestamp": now(),
        })

    return mutate_state(state_path, transform)


def main():
    parser = argparse.ArgumentParser(description="Record a filed issue in FLOW state")
    parser.add_argument("--label", required=True, help="Issue label (e.g. Rule, Flow, Flaky Test)")
    parser.add_argument("--title", required=True, help="Issue title")
    parser.add_argument("--url", required=True, help="Issue URL")
    parser.add_argument("--phase", required=True, help="Phase that filed the issue")
    parser.add_argument("--branch", type=str, default=None, help="Override branch for state file lookup")
    args = parser.parse_args()

    root = project_root()
    branch, candidates = resolve_branch(args.branch)

    if branch is None:
        if candidates:
            print(json.dumps({
                "status": "error",
                "message": "Multiple active features. Pass --branch.",
                "candidates": candidates,
            }))
        else:
            print(json.dumps({
                "status": "error",
                "message": "Could not determine current branch",
            }))
        sys.exit(1)

    state_path = root / ".flow-states" / f"{branch}.json"

    if not state_path.exists():
        print(json.dumps({"status": "no_state"}))
        sys.exit(0)

    try:
        state = add_issue(state_path, args.label, args.title, args.url, args.phase)
    except (json.JSONDecodeError, FileNotFoundError) as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Failed to add issue: {e}",
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "issue_count": len(state["issues_filed"]),
    }))


if __name__ == "__main__":
    main()
