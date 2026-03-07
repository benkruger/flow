"""Phase entry and completion state transitions.

Handles the two standard mutations every phase skill performs:
entering a phase and completing a phase.

Usage:
  bin/flow phase-transition --phase <name> --action enter
  bin/flow phase-transition --phase <name> --action complete [--next-phase <name>]

Output (JSON to stdout):
  Enter:    {"status": "ok", "phase": "plan", "action": "enter", "visit_count": 1, "first_visit": true}
  Complete: {"status": "ok", "phase": "plan", "action": "complete", "cumulative_seconds": 300, "formatted_time": "5m", "next_phase": "code"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PACIFIC, current_branch, format_time, now, project_root, PHASE_ORDER


def _parse_timestamp(ts):
    """Parse an ISO 8601 timestamp string to a timezone-aware datetime."""
    return datetime.fromisoformat(ts)


def phase_enter(state, phase):
    """Apply phase entry mutations. Returns (state, result_dict)."""
    phase_data = state["phases"][phase]

    phase_data["status"] = "in_progress"
    if phase_data["started_at"] is None:
        phase_data["started_at"] = now()
    phase_data["session_started_at"] = now()
    phase_data["visit_count"] = phase_data.get("visit_count", 0) + 1
    state["current_phase"] = phase

    first_visit = phase_data["visit_count"] == 1

    return state, {
        "status": "ok",
        "phase": phase,
        "action": "enter",
        "visit_count": phase_data["visit_count"],
        "first_visit": first_visit,
    }


def phase_complete(state, phase, next_phase=None):
    """Apply phase completion mutations. Returns (state, result_dict)."""
    phase_data = state["phases"][phase]

    if next_phase is None:
        phase_idx = PHASE_ORDER.index(phase)
        next_phase = PHASE_ORDER[phase_idx + 1]

    session_started = phase_data.get("session_started_at")
    if session_started:
        started_dt = _parse_timestamp(session_started)
        now_dt = datetime.now(PACIFIC)
        elapsed = int((now_dt - started_dt).total_seconds())
        if elapsed < 0:
            elapsed = 0
    else:
        elapsed = 0

    existing = phase_data.get("cumulative_seconds", 0)
    cumulative = existing + elapsed

    phase_data["cumulative_seconds"] = cumulative
    phase_data["status"] = "complete"
    phase_data["completed_at"] = now()
    phase_data["session_started_at"] = None
    state["current_phase"] = next_phase

    return state, {
        "status": "ok",
        "phase": phase,
        "action": "complete",
        "cumulative_seconds": cumulative,
        "formatted_time": format_time(cumulative),
        "next_phase": next_phase,
    }


# Phases that support entry/completion via this script (all except cleanup)
_VALID_PHASES = PHASE_ORDER[:-1]


def main():
    parser = argparse.ArgumentParser(description="Phase entry/completion transitions")
    parser.add_argument("--phase", type=str, required=True,
                        help="Phase name (e.g. start, plan, code)")
    parser.add_argument("--action", required=True, choices=["enter", "complete"],
                        help="Action: enter or complete")
    parser.add_argument("--next-phase", type=str, default=None,
                        help="Override next phase name (default: next in order)")
    args = parser.parse_args()

    if args.phase not in _VALID_PHASES:
        print(json.dumps({
            "status": "error",
            "message": f"Invalid phase: {args.phase}. Must be one of: {', '.join(_VALID_PHASES)}",
        }))
        sys.exit(1)

    root = project_root()
    branch = current_branch()

    if not branch:
        print(json.dumps({
            "status": "error",
            "message": "Could not determine current branch",
        }))
        sys.exit(1)

    state_path = root / ".flow-states" / f"{branch}.json"

    if not state_path.exists():
        print(json.dumps({
            "status": "error",
            "message": f"No state file found: {state_path}",
        }))
        sys.exit(1)

    try:
        state = json.loads(state_path.read_text())
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)

    if "phases" not in state or args.phase not in state["phases"]:
        print(json.dumps({
            "status": "error",
            "message": f"Phase {args.phase} not found in state file",
        }))
        sys.exit(1)

    if args.action == "enter":
        state, result = phase_enter(state, args.phase)
    else:
        state, result = phase_complete(state, args.phase, args.next_phase)

    state_path.write_text(json.dumps(state, indent=2))
    print(json.dumps(result))


if __name__ == "__main__":
    main()
