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

from flow_utils import current_branch, project_root, PHASE_NAMES

COMMANDS = {
    "1": "/flow:start",     "2": "/flow:plan",      "3": "/flow:code",
    "4": "/flow:simplify",  "5": "/flow:review",    "6": "/flow:security",
    "7": "/flow:reflect",   "8": "/flow:cleanup",
}


def check_phase(state, phase):
    """Check if entry into `phase` is allowed given the state dict.

    Returns (allowed: bool, output: str) where output is the message to print.
    output is empty string if allowed with no note.
    """
    prev = phase - 1
    prev_str = str(prev)
    prev_data = state.get("phases", {}).get(prev_str, {})
    prev_status = prev_data.get("status", "pending")
    prev_name = PHASE_NAMES.get(prev, f"Phase {prev}")
    prev_cmd = COMMANDS.get(prev_str, f"/flow:phase{prev}")

    if prev_status != "complete":
        lines = [
            f"BLOCKED: Phase {prev}: {prev_name} must be complete before "
            f"entering Phase {phase}: {PHASE_NAMES.get(phase, '')}.",
            f"Phase {prev} current status: {prev_status}",
            f"Complete it first with: {prev_cmd}",
        ]
        return (False, "\n".join(lines))

    # Allowed — note if revisiting
    this_data = state.get("phases", {}).get(str(phase), {})
    if this_data.get("status") == "complete":
        visits = this_data.get("visit_count", 0)
        name = PHASE_NAMES.get(phase, f"Phase {phase}")
        return (True, f"NOTE: Phase {phase}: {name} was previously completed "
                      f"({visits} visit(s)). Re-entering.")

    return (True, "")


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

    allowed, output = check_phase(state, phase)
    if output:
        print(output)
    sys.exit(0 if allowed else 1)


if __name__ == "__main__":
    main()
