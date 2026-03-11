"""Format phase timings as a markdown table for PR body.

Usage: bin/flow format-pr-timings --state-file <path> --output <path>

Output (JSON to stdout):
  Success: {"status": "ok", "output": "<path>"}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import format_time, PHASE_NAMES, PHASE_ORDER


def format_timings_table(state):
    """Build a markdown timings table from state dict."""
    phases = state.get("phases", {})
    lines = [
        "| Phase | Duration |",
        "|-------|----------|",
    ]

    total_seconds = 0
    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        name = PHASE_NAMES.get(key, key)
        seconds = phase.get("cumulative_seconds", 0)
        total_seconds += seconds
        lines.append(f"| {name} | {format_time(seconds)} |")

    lines.append(f"| **Total** | **{format_time(total_seconds)}** |")

    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Format phase timings table")
    parser.add_argument("--state-file", required=True, help="Path to state JSON file")
    parser.add_argument("--output", required=True, help="Path to write markdown output")

    args = parser.parse_args()

    try:
        state_path = Path(args.state_file)
        if not state_path.exists():
            print(json.dumps({"status": "error", "message": f"State file not found: {args.state_file}"}))
            return

        state = json.loads(state_path.read_text())
        table = format_timings_table(state)

        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(table)

        print(json.dumps({"status": "ok", "output": args.output}))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
