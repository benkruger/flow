"""Record a sent Slack notification in the FLOW state file.

Usage:
  bin/flow add-notification --phase <phase> --ts <ts> --thread-ts <thread_ts>
                            --message <text> [--branch <branch>]

Appends to the slack_notifications array in the state file. Follows the same
pattern as add-issue.py for state file discovery and mutation.

Output (JSON to stdout):
  Success:  {"status": "ok", "notification_count": N}
  No state: {"status": "no_state"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PHASE_NAMES, mutate_state, now, project_root, resolve_branch

MAX_PREVIEW_LENGTH = 100


def add_notification(state_path, phase, ts, thread_ts, message):
    """Append a notification to the state file. Returns the updated state dict."""
    preview = message
    if len(preview) > MAX_PREVIEW_LENGTH:
        preview = preview[:MAX_PREVIEW_LENGTH] + "..."

    def transform(state):
        if "slack_notifications" not in state:
            state["slack_notifications"] = []
        state["slack_notifications"].append(
            {
                "phase": phase,
                "phase_name": PHASE_NAMES.get(phase, phase),
                "ts": ts,
                "thread_ts": thread_ts,
                "message_preview": preview,
                "timestamp": now(),
            }
        )

    return mutate_state(state_path, transform)


def main():
    parser = argparse.ArgumentParser(description="Record a Slack notification in FLOW state")
    parser.add_argument("--phase", required=True, help="Phase that sent the notification")
    parser.add_argument("--ts", required=True, help="Slack message timestamp")
    parser.add_argument("--thread-ts", required=True, help="Slack thread timestamp")
    parser.add_argument("--message", required=True, help="Message text (truncated for preview)")
    parser.add_argument("--branch", type=str, default=None, help="Override branch for state file lookup")
    args = parser.parse_args()

    root = project_root()
    branch, candidates = resolve_branch(args.branch)

    if branch is None:
        if candidates:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": "Multiple active features. Pass --branch.",
                        "candidates": candidates,
                    }
                )
            )
        else:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": "Could not determine current branch",
                    }
                )
            )
        sys.exit(1)

    state_path = root / ".flow-states" / f"{branch}.json"

    if not state_path.exists():
        print(json.dumps({"status": "no_state"}))
        sys.exit(0)

    try:
        state = add_notification(state_path, args.phase, args.ts, args.thread_ts, args.message)
    except (json.JSONDecodeError, FileNotFoundError) as exc:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Could not read state file: {exc}",
                }
            )
        )
        sys.exit(1)
    except Exception as exc:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Failed to add notification: {exc}",
                }
            )
        )
        sys.exit(1)

    print(
        json.dumps(
            {
                "status": "ok",
                "notification_count": len(state["slack_notifications"]),
            }
        )
    )


if __name__ == "__main__":
    main()
