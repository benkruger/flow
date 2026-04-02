"""Update the Start phase step counter in the FLOW state file.

Combines step tracking with subcommand execution in a single tool call.
When wrapping a subcommand, updates the counter then execs the subcommand
via bin/flow. Best-effort: silently skips if the state file is missing
or corrupt.

Usage:
  bin/flow start-step --step 5 --branch my-feature
  bin/flow start-step --step 6 --branch my-feature -- ci --branch main

Output (JSON to stdout when standalone):
  Updated:  {"status": "ok", "step": 5}
  Skipped:  {"status": "skipped", "reason": "no state file"}
  Wrapping: (subcommand stdout — no JSON from start-step)
"""

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, project_root


def update_step(state_path, step):
    """Update start_step in the state file. Returns True if updated."""
    if not state_path.exists():
        return False

    def _update(state):
        state["start_step"] = step

    mutate_state(state_path, _update)
    return True


def main():
    # Split at '--' before argparse sees it
    argv = sys.argv[1:]
    if "--" in argv:
        sep = argv.index("--")
        own_args = argv[:sep]
        subcommand = argv[sep + 1 :]
    else:
        own_args = argv
        subcommand = []

    parser = argparse.ArgumentParser(description="Update Start phase step counter")
    parser.add_argument("--step", type=int, required=True, help="Step number to set")
    parser.add_argument("--branch", required=True, help="Branch name for state file lookup")
    args = parser.parse_args(own_args)

    root = project_root()
    state_path = root / ".flow-states" / f"{args.branch}.json"

    try:
        updated = update_step(state_path, args.step)
    except Exception:
        updated = False

    if subcommand:
        flow_bin = str(Path(__file__).resolve().parent.parent / "bin" / "flow")
        os.execvp(flow_bin, [flow_bin] + subcommand)
    elif updated:
        print(json.dumps({"status": "ok", "step": args.step}))
    else:
        print(json.dumps({"status": "skipped", "reason": "no state file"}))


if __name__ == "__main__":
    main()
