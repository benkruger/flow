"""Manage orchestration queue state at .flow-states/orchestrate.json.

Usage:
  bin/flow orchestrate-state --create --queue-file <path> --state-dir <dir>
  bin/flow orchestrate-state --start-issue <index> --state-file <path>
  bin/flow orchestrate-state --record-outcome <index> --outcome <completed|failed> --state-file <path>
  bin/flow orchestrate-state --complete --state-file <path>
  bin/flow orchestrate-state --read --state-file <path>
  bin/flow orchestrate-state --next --state-file <path>

Output (JSON to stdout):
  Success: {"status": "ok", ...}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, now


def _build_queue_item(issue):
    """Build a normalized queue item from an issue dict."""
    return {
        "issue_number": issue["issue_number"],
        "title": issue["title"],
        "status": "pending",
        "started_at": None,
        "completed_at": None,
        "outcome": None,
        "pr_url": None,
        "branch": None,
        "reason": None,
    }


def create_state(queue, state_dir):
    """Create orchestrate.json with the given issue queue.

    Returns dict with status. Refuses to overwrite an in-progress orchestration.
    """
    state_dir_path = Path(state_dir)
    state_dir_path.mkdir(parents=True, exist_ok=True)
    state_path = state_dir_path / "orchestrate.json"

    if state_path.exists():
        existing = json.loads(state_path.read_text())
        if existing.get("completed_at") is None:
            return {
                "status": "error",
                "message": "Orchestration already in progress. Complete or abort the current run first.",
            }

    state = {
        "started_at": now(),
        "completed_at": None,
        "queue": [_build_queue_item(issue) for issue in queue],
        "current_index": None,
    }

    state_path.write_text(json.dumps(state, indent=2))
    return {"status": "ok"}


def start_issue(state_path, index):
    """Mark queue item at index as in_progress."""
    path = Path(state_path)
    if not path.exists():
        return {"status": "error", "message": f"State file not found: {state_path}"}

    result = {"status": "ok"}

    def transform(s):
        queue = s.get("queue", [])
        if index < 0 or index >= len(queue):
            result["status"] = "error"
            result["message"] = f"Index {index} out of range (queue has {len(queue)} items)"
            return
        s["current_index"] = index
        s["queue"][index]["status"] = "in_progress"
        s["queue"][index]["started_at"] = now()

    mutate_state(state_path, transform)
    return result


def record_outcome(state_path, index, outcome, pr_url=None, branch=None, reason=None):
    """Record the outcome for a queue item."""
    path = Path(state_path)
    if not path.exists():
        return {"status": "error", "message": f"State file not found: {state_path}"}

    result = {"status": "ok"}

    def transform(s):
        queue = s.get("queue", [])
        if index < 0 or index >= len(queue):
            result["status"] = "error"
            result["message"] = f"Index {index} out of range (queue has {len(queue)} items)"
            return
        item = s["queue"][index]
        item["status"] = outcome
        item["outcome"] = outcome
        item["completed_at"] = now()
        if pr_url:
            item["pr_url"] = pr_url
        if branch:
            item["branch"] = branch
        if reason:
            item["reason"] = reason

    mutate_state(state_path, transform)
    return result


def complete_orchestration(state_path):
    """Mark orchestration as complete."""
    path = Path(state_path)
    if not path.exists():
        return {"status": "error", "message": f"State file not found: {state_path}"}

    def transform(s):
        s["completed_at"] = now()

    mutate_state(state_path, transform)
    return {"status": "ok"}


def read_state(state_path):
    """Read and return the current orchestration state."""
    path = Path(state_path)
    if not path.exists():
        return {"status": "error", "message": f"State file not found: {state_path}"}

    state = json.loads(path.read_text())
    return {"status": "ok", "state": state}


def next_issue(state_path):
    """Find the next pending issue in the queue."""
    path = Path(state_path)
    if not path.exists():
        return {"status": "error", "message": f"State file not found: {state_path}"}

    state = json.loads(path.read_text())
    queue = state.get("queue", [])

    for i, item in enumerate(queue):
        if item["status"] == "pending":
            return {
                "status": "ok",
                "index": i,
                "issue_number": item["issue_number"],
                "title": item["title"],
            }

    return {"status": "done"}


def main():
    parser = argparse.ArgumentParser(description="Manage orchestration queue state")

    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--create", action="store_true", help="Create orchestrate.json")
    group.add_argument("--start-issue", type=int, metavar="INDEX", help="Mark issue as in_progress")
    group.add_argument("--record-outcome", type=int, metavar="INDEX", help="Record outcome for issue")
    group.add_argument("--complete", action="store_true", help="Mark orchestration complete")
    group.add_argument("--read", action="store_true", help="Read current state")
    group.add_argument("--next", action="store_true", help="Get next pending issue")

    parser.add_argument("--queue-file", help="Path to JSON file with issue queue")
    parser.add_argument("--state-dir", help="Path to .flow-states/ directory")
    parser.add_argument("--state-file", help="Path to orchestrate.json")
    parser.add_argument("--outcome", choices=["completed", "failed"], help="Outcome for --record-outcome")
    parser.add_argument("--pr-url", help="PR URL for completed issues")
    parser.add_argument("--branch", help="Branch name for completed issues")
    parser.add_argument("--reason", help="Failure reason for failed issues")

    args = parser.parse_args()

    try:
        if args.create:
            if not args.queue_file:
                print(json.dumps({"status": "error", "message": "--queue-file required with --create"}))
                return
            state_dir = args.state_dir or ".flow-states"
            queue = json.loads(Path(args.queue_file).read_text())
            result = create_state(queue, state_dir)

        elif args.start_issue is not None:
            if not args.state_file:
                print(json.dumps({"status": "error", "message": "--state-file required"}))
                return
            result = start_issue(args.state_file, args.start_issue)

        elif args.record_outcome is not None:
            if not args.state_file:
                print(json.dumps({"status": "error", "message": "--state-file required"}))
                return
            if not args.outcome:
                print(json.dumps({"status": "error", "message": "--outcome required with --record-outcome"}))
                return
            result = record_outcome(
                args.state_file,
                args.record_outcome,
                args.outcome,
                pr_url=args.pr_url,
                branch=args.branch,
                reason=args.reason,
            )

        elif args.complete:
            if not args.state_file:
                print(json.dumps({"status": "error", "message": "--state-file required"}))
                return
            result = complete_orchestration(args.state_file)

        elif args.read:
            if not args.state_file:
                print(json.dumps({"status": "error", "message": "--state-file required"}))
                return
            result = read_state(args.state_file)

        elif args.next:
            if not args.state_file:
                print(json.dumps({"status": "error", "message": "--state-file required"}))
                return
            result = next_issue(args.state_file)

        print(json.dumps(result))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
