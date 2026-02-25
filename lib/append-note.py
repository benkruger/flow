"""Append a structured note to the FLOW state file.

Usage:
  bin/flow append-note <state_file_path> --phase <N> --type <correction|learning> --note "text"

Output (JSON to stdout):
  Success: {"status": "ok", "note_count": N}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

PHASE_NAMES = {
    1: "Start", 2: "Research", 3: "Design", 4: "Plan",
    5: "Code", 6: "Review", 7: "Reflect", 8: "Cleanup",
}


def _now():
    """Return current UTC timestamp in ISO 8601 format."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def append_note(state_path, phase, note_type, note_text):
    """Append a note to the state file. Returns the updated state dict."""
    state = json.loads(state_path.read_text())

    if "notes" not in state:
        state["notes"] = []

    state["notes"].append({
        "phase": phase,
        "phase_name": PHASE_NAMES.get(phase, f"Phase {phase}"),
        "timestamp": _now(),
        "type": note_type,
        "note": note_text,
    })

    state_path.write_text(json.dumps(state, indent=2))
    return state


def main():
    parser = argparse.ArgumentParser(description="Append a note to FLOW state")
    parser.add_argument("state_file", help="Path to the state file")
    parser.add_argument("--phase", type=int, required=True,
                        help="Current phase number")
    parser.add_argument("--type", dest="note_type", required=True,
                        choices=["correction", "learning"],
                        help="Note type")
    parser.add_argument("--note", required=True,
                        help="Note text")
    args = parser.parse_args()

    state_path = Path(args.state_file)

    if not state_path.exists():
        print(json.dumps({
            "status": "error",
            "message": f"State file not found: {args.state_file}",
        }))
        sys.exit(1)

    try:
        state = append_note(state_path, args.phase, args.note_type, args.note)
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
