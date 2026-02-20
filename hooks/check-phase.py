#!/usr/bin/env python3
"""
FLOW Phase Entry Guard

Usage:
  python3 hooks/check-phase.py --required <phase_number>

Checks that the previous phase is complete before allowing entry into
the requested phase. Reads .claude/flow-states/<branch>.json from the
project root. Works correctly whether run from the project root or from
inside a worktree.

Exit 0 — entry allowed
Exit 1 — entry blocked (error printed to stdout for Claude to read)
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

PHASES = {
    "1": "Start",     "2": "Research",  "3": "Design",
    "4": "Plan",      "5": "Implement", "6": "Review",
    "7": "Reflect",   "8": "Cleanup"
}

COMMANDS = {
    "1": "/flow:start",     "2": "/flow:research",  "3": "/flow:design",
    "4": "/flow:plan",      "5": "/flow:implement",  "6": "/flow:review",
    "7": "/flow:reflect",   "8": "/flow:cleanup"
}


def project_root():
    try:
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            capture_output=True, text=True, check=True
        )
        for line in result.stdout.strip().split("\n"):
            if line.startswith("worktree "):
                return Path(line.split(" ", 1)[1].strip())
    except Exception:
        pass
    return Path(".")


def current_branch():
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            capture_output=True, text=True, check=True
        )
        return result.stdout.strip()
    except Exception:
        return None


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
    state_file = root / ".claude" / "ror-states" / f"{branch}.json"

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
