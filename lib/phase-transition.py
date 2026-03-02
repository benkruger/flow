"""Phase entry and completion state transitions.

Handles the two standard mutations every phase skill performs:
entering a phase and completing a phase.

Usage:
  bin/flow phase-transition --phase <N> --action enter
  bin/flow phase-transition --phase <N> --action complete [--next-phase <M>]

Output (JSON to stdout):
  Enter:    {"status": "ok", "phase": 2, "action": "enter", "visit_count": 1, "first_visit": true}
  Complete: {"status": "ok", "phase": 2, "action": "complete", "cumulative_seconds": 300, "formatted_time": "5m", "next_phase": 3}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PACIFIC, current_branch, format_time, now, project_root


def _parse_timestamp(ts):
    """Parse an ISO 8601 timestamp string to a timezone-aware datetime."""
    return datetime.fromisoformat(ts)


def phase_enter(state, phase):
    """Apply phase entry mutations. Returns (state, result_dict)."""
    phase_str = str(phase)
    phase_data = state["phases"][phase_str]

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
    phase_str = str(phase)
    phase_data = state["phases"][phase_str]

    if next_phase is None:
        next_phase = phase + 1

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


def main():
    parser = argparse.ArgumentParser(description="Phase entry/completion transitions")
    parser.add_argument("--phase", type=int, required=True,
                        help="Phase number (1-7)")
    parser.add_argument("--action", required=True, choices=["enter", "complete"],
                        help="Action: enter or complete")
    parser.add_argument("--next-phase", type=int, default=None,
                        help="Override next phase number (default: phase + 1)")
    args = parser.parse_args()

    if args.phase < 1 or args.phase > 7:
        print(json.dumps({
            "status": "error",
            "message": f"Invalid phase number: {args.phase}. Must be 1-7.",
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

    phase_str = str(args.phase)
    if "phases" not in state or phase_str not in state["phases"]:
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
