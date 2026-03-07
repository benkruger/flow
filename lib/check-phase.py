#!/usr/bin/env python3
"""
FLOW Phase Entry Guard

Usage:
  bin/flow check-phase --required <phase_name>

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

from flow_utils import current_branch, project_root, COMMANDS, PHASE_NAMES, PHASE_NUMBER, PHASE_ORDER


def check_phase(state, phase):
    """Check if entry into `phase` is allowed given the state dict.

    Returns (allowed: bool, output: str) where output is the message to print.
    output is empty string if allowed with no note.
    """
    phase_idx = PHASE_ORDER.index(phase)
    prev = PHASE_ORDER[phase_idx - 1]
    prev_data = state.get("phases", {}).get(prev, {})
    prev_status = prev_data.get("status", "pending")
    prev_name = PHASE_NAMES.get(prev, prev)
    prev_num = PHASE_NUMBER.get(prev, "?")
    prev_cmd = COMMANDS.get(prev, f"/flow:{prev}")

    phase_name = PHASE_NAMES.get(phase, phase)
    phase_num = PHASE_NUMBER.get(phase, "?")

    if prev_status != "complete":
        lines = [
            f"BLOCKED: Phase {prev_num}: {prev_name} must be complete before "
            f"entering Phase {phase_num}: {phase_name}.",
            f"Phase {prev_num} current status: {prev_status}",
            f"Complete it first with: {prev_cmd}",
        ]
        return (False, "\n".join(lines))

    # Allowed — note if revisiting
    this_data = state.get("phases", {}).get(phase, {})
    if this_data.get("status") == "complete":
        visits = this_data.get("visit_count", 0)
        return (True, f"NOTE: Phase {phase_num}: {phase_name} was previously completed "
                      f"({visits} visit(s)). Re-entering.")

    return (True, "")


def main():
    parser = argparse.ArgumentParser(description="SDLC phase entry guard")
    parser.add_argument("--required", type=str, required=True,
                        help="Phase name being entered")
    args = parser.parse_args()
    phase = args.required

    # First phase has no prerequisites
    if phase == PHASE_ORDER[0]:
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
