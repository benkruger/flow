#!/usr/bin/env python3
"""
FLOW Phase Entry Guard

Usage:
  bin/flow check-phase --required <phase_number>

Checks that the previous phase is complete before allowing entry into
the requested phase. Reads .flow-states/<branch>.json from the
project root. Works correctly whether run from the project root or from
inside a worktree.

Exit 0 — entry allowed
Exit 1 — entry blocked (error printed to stdout for Claude to read)
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import current_branch, project_root

PHASES = {
    "1": "Start",     "2": "Research",  "3": "Design",
    "4": "Plan",      "5": "Code", "6": "Review",
    "7": "Reflect",   "8": "Cleanup"
}

COMMANDS = {
    "1": "/flow:start",     "2": "/flow:research",  "3": "/flow:design",
    "4": "/flow:plan",      "5": "/flow:code",  "6": "/flow:review",
    "7": "/flow:reflect",   "8": "/flow:cleanup"
}


def main():
    parser = argparse.ArgumentParser(description="SDLC phase entry guard")
    parser.add_argument("--required", type=int, required=True,
                        help="Phase number being entered")
    args = parser.parse_args()
    phase = args.required

    # Phase 1 has no prerequisites
    if phase == 1:
        sys.exit(0)

    branch = current_branch()
    if not branch:
        print("BLOCKED: Could not determine current git branch.")
        sys.exit(1)

    root = project_root()
    state_file = root / ".flow-states" / f"{branch}.json"

    if not state_file.exists():
        print(f'BLOCKED: No FLOW feature in progress on branch "{branch}".')
        print("Run /flow:start to begin a new feature.")
        sys.exit(1)

    try:
        state = json.loads(state_file.read_text())
    except Exception as e:
        print(f"BLOCKED: Could not read state file: {e}")
        sys.exit(1)

    prev = str(phase - 1)
    prev_data = state.get("phases", {}).get(prev, {})
    prev_status = prev_data.get("status", "pending")
    prev_name = PHASES.get(prev, f"Phase {prev}")
    prev_cmd = COMMANDS.get(prev, f"/flow:phase{prev}")

    if prev_status != "complete":
        print(f"BLOCKED: Phase {prev}: {prev_name} must be complete before "
              f"entering Phase {phase}: {PHASES.get(str(phase), '')}.")
        print(f"Phase {prev} current status: {prev_status}")
        print(f"Complete it first with: {prev_cmd}")
        sys.exit(1)

    # Allowed — note if revisiting
    this_data = state.get("phases", {}).get(str(phase), {})
    if this_data.get("status") == "complete":
        visits = this_data.get("visit_count", 0)
        name = PHASES.get(str(phase), f"Phase {phase}")
        print(f"NOTE: Phase {phase}: {name} was previously completed "
              f"({visits} visit(s)). Re-entering.")

    sys.exit(0)


if __name__ == "__main__":
    main()
