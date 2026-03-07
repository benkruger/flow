"""Append a structured note to the FLOW state file.

Usage:
  bin/flow append-note --note "text" [--type correction|learning]

Derives state file path and current phase from git context.
Type defaults to "correction".

Output (JSON to stdout):
  Success:  {"status": "ok", "note_count": N}
  No state: {"status": "no_state"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import current_branch, now, project_root, PHASE_NAMES


def append_note(state_path, phase, note_type, note_text):
    """Append a note to the state file. Returns the updated state dict."""
    state = json.loads(state_path.read_text())

    if "notes" not in state:
        state["notes"] = []

    state["notes"].append({
        "phase": phase,
        "phase_name": PHASE_NAMES.get(phase, phase),
        "timestamp": now(),
        "type": note_type,
        "note": note_text,
    })

    state_path.write_text(json.dumps(state, indent=2))
    return state


def main():
    parser = argparse.ArgumentParser(description="Append a note to FLOW state")
    parser.add_argument("--type", dest="note_type", default="correction",
                        choices=["correction", "learning"],
                        help="Note type (default: correction)")
    parser.add_argument("--note", required=True,
                        help="Note text")
    args = parser.parse_args()

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
        print(json.dumps({"status": "no_state"}))
        sys.exit(0)

    try:
        state_data = json.loads(state_path.read_text())
        phase = state_data.get("current_phase", "start")
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)

    try:
        state = append_note(state_path, phase, args.note_type, args.note)
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Failed to append note: {e}",
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "note_count": len(state["notes"]),
    }))


if __name__ == "__main__":
    main()
