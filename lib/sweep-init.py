"""Initialize the FLOW sweep state file.

Usage:
  bin/flow sweep-init --issues '<JSON array>'
  bin/flow sweep-init --issues '[{"number":42,"title":"Fix bug"},{"number":43,"title":"Add feature"}]'

Options:
  --limit <N>  Concurrency limit (default: 3)

Creates .flow-states/sweep.json with all issues set to "queued".
Fails if sweep.json already exists (use --force to overwrite).

Output (JSON to stdout):
  Success:  {"status": "ok", "issue_count": N, "path": "..."}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, project_root


def create_sweep(sweep_path, issues_data, concurrency_limit=3):
    """Create a new sweep state file.

    issues_data is a list of dicts with at least "number" and "title".
    Returns the sweep dict.
    """
    issues = []
    for item in issues_data:
        issues.append({
            "number": item["number"],
            "title": item["title"],
            "status": "queued",
            "branch": None,
            "worktree": None,
            "pr_number": None,
            "pr_url": None,
            "agent_name": f"worker-{item['number']}",
            "started_at": None,
            "completed_at": None,
            "error": None,
        })

    sweep = {
        "started_at": now(),
        "status": "in_progress",
        "concurrency_limit": concurrency_limit,
        "issues": issues,
    }

    sweep_path.parent.mkdir(parents=True, exist_ok=True)
    sweep_path.write_text(json.dumps(sweep, indent=2))
    return sweep


def main():
    parser = argparse.ArgumentParser(description="Initialize sweep state")
    parser.add_argument("--issues", required=True,
                        help="JSON array of {number, title} objects")
    parser.add_argument("--limit", type=int, default=3,
                        help="Concurrency limit (default: 3)")
    parser.add_argument("--force", action="store_true",
                        help="Overwrite existing sweep.json")
    args = parser.parse_args()

    root = project_root()
    sweep_path = root / ".flow-states" / "sweep.json"

    if sweep_path.exists() and not args.force:
        print(json.dumps({
            "status": "error",
            "message": "sweep.json already exists. Use --force to overwrite.",
        }))
        sys.exit(1)

    try:
        issues_data = json.loads(args.issues)
    except json.JSONDecodeError as e:
        print(json.dumps({
            "status": "error",
            "message": f"Invalid JSON for --issues: {e}",
        }))
        sys.exit(1)

    if not isinstance(issues_data, list) or not issues_data:
        print(json.dumps({
            "status": "error",
            "message": "--issues must be a non-empty JSON array",
        }))
        sys.exit(1)

    try:
        create_sweep(sweep_path, issues_data, args.limit)
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Failed to create sweep.json: {e}",
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "issue_count": len(issues_data),
        "path": str(sweep_path),
    }))


if __name__ == "__main__":
    main()
