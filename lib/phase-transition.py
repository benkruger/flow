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
import subprocess
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    COMMANDS, PACIFIC, format_time, load_phase_config, now, project_root,
    resolve_branch, PHASE_ORDER,
)


def _capture_diff_stats():
    """Capture git diff --stat summary for the current branch vs main.

    Returns a dict with files_changed, insertions, deletions, captured_at.
    Best-effort: returns zeros if git fails.
    """
    try:
        result = subprocess.run(
            ["git", "diff", "--stat", "main...HEAD"],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            return {"files_changed": 0, "insertions": 0, "deletions": 0, "captured_at": now()}
        lines = result.stdout.strip().split("\n")
        summary = lines[-1]
        files_changed = 0
        insertions = 0
        deletions = 0
        for part in summary.split(","):
            part = part.strip()
            if "file" in part:
                files_changed = int(part.split()[0])
            elif "insertion" in part:
                insertions = int(part.split()[0])
            elif "deletion" in part:
                deletions = int(part.split()[0])
        return {
            "files_changed": files_changed,
            "insertions": insertions,
            "deletions": deletions,
            "captured_at": now(),
        }
    except Exception:
        return {"files_changed": 0, "insertions": 0, "deletions": 0, "captured_at": now()}


def _parse_timestamp(ts):
    """Parse an ISO 8601 timestamp string to a timezone-aware datetime."""
    return datetime.fromisoformat(ts)


def phase_enter(state, phase, reason=None):
    """Apply phase entry mutations. Returns (state, result_dict)."""
    prev_phase = state.get("current_phase")
    phase_data = state["phases"][phase]

    phase_data["status"] = "in_progress"
    if phase_data["started_at"] is None:
        phase_data["started_at"] = now()
    phase_data["session_started_at"] = now()
    phase_data["visit_count"] = phase_data.get("visit_count", 0) + 1
    state["current_phase"] = phase

    transition = {"from": prev_phase, "to": phase, "timestamp": now()}
    if reason:
        transition["reason"] = reason
    state.setdefault("phase_transitions", []).append(transition)

    if phase == "flow-code-review":
        state["code_review_step"] = 0

    # Clear auto-continue flag from the previous phase
    state.pop("_auto_continue", None)

    first_visit = phase_data["visit_count"] == 1

    return state, {
        "status": "ok",
        "phase": phase,
        "action": "enter",
        "visit_count": phase_data["visit_count"],
        "first_visit": first_visit,
    }


def phase_complete(state, phase, next_phase=None, phase_order=None,
                    phase_commands=None):
    """Apply phase completion mutations. Returns (state, result_dict)."""
    phase_data = state["phases"][phase]

    if next_phase is None:
        order = phase_order or PHASE_ORDER
        phase_idx = order.index(phase)
        next_phase = order[phase_idx + 1]

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

    # Set _auto_continue if the current phase has continue=auto
    skills = state.get("skills", {})
    skill_config = skills.get(phase, {})
    if isinstance(skill_config, str):
        continue_mode = skill_config
    elif isinstance(skill_config, dict):
        continue_mode = skill_config.get("continue")
    else:
        continue_mode = None

    if continue_mode == "auto":
        commands = phase_commands or COMMANDS
        next_command = commands.get(next_phase)
        if next_command:
            state["_auto_continue"] = next_command
    else:
        state.pop("_auto_continue", None)

    if phase == "flow-code":
        state["diff_stats"] = _capture_diff_stats()

    return state, {
        "status": "ok",
        "phase": phase,
        "action": "complete",
        "cumulative_seconds": cumulative,
        "formatted_time": format_time(cumulative),
        "next_phase": next_phase,
    }


# Phases that support entry/completion via this script (all except complete)
_VALID_PHASES = PHASE_ORDER[:-1]


def main():
    parser = argparse.ArgumentParser(description="Phase entry/completion transitions")
    parser.add_argument("--phase", type=str, required=True,
                        help="Phase name (e.g. start, plan, code)")
    parser.add_argument("--action", required=True, choices=["enter", "complete"],
                        help="Action: enter or complete")
    parser.add_argument("--next-phase", type=str, default=None,
                        help="Override next phase name (default: next in order)")
    parser.add_argument("--branch", type=str, default=None,
                        help="Override branch for state file lookup")
    parser.add_argument("--reason", type=str, default=None,
                        help="Optional reason for backward transitions")
    args = parser.parse_args()

    if args.phase not in _VALID_PHASES:
        print(json.dumps({
            "status": "error",
            "message": f"Invalid phase: {args.phase}. Must be one of: {', '.join(_VALID_PHASES)}",
        }))
        sys.exit(1)

    root = project_root()
    branch, candidates = resolve_branch(args.branch)

    if branch is None:
        if candidates:
            print(json.dumps({
                "status": "error",
                "message": "Multiple active features. Pass --branch.",
                "candidates": candidates,
            }))
        else:
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

    # Load frozen phase config if available, fall back to module-level constants
    frozen_path = root / ".flow-states" / f"{branch}-phases.json"
    frozen_order = None
    frozen_commands = None
    if frozen_path.exists():
        frozen_order, _, _, frozen_commands = load_phase_config(frozen_path)

    if args.action == "enter":
        state, result = phase_enter(state, args.phase, reason=args.reason)
    else:
        state, result = phase_complete(
            state, args.phase, args.next_phase,
            phase_order=frozen_order,
            phase_commands=frozen_commands,
        )

    state_path.write_text(json.dumps(state, indent=2))
    print(json.dumps(result))


if __name__ == "__main__":
    main()
