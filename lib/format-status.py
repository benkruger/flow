"""Format the FLOW status panel.

Usage: bin/flow format-status <state_file_path> <plugin_version>

Reads the state file and outputs the formatted status panel text to stdout.
The skill wraps the output in a fenced code block.

Output (JSON to stdout):
  Success: {"status": "ok", "panel": "..."}
  No state: {"status": "no_state"}
  Error:   {"status": "error", "message": "..."}
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import format_time

PHASE_NAMES = {
    1: "Start", 2: "Research", 3: "Design", 4: "Plan",
    5: "Code", 6: "Review", 7: "Reflect", 8: "Cleanup",
}

COMMANDS = {
    1: "/flow:start", 2: "/flow:research", 3: "/flow:design",
    4: "/flow:plan", 5: "/flow:code", 6: "/flow:review",
    7: "/flow:reflect", 8: "/flow:cleanup",
}

# Column width for phase name alignment
NAME_WIDTH = 12


def format_panel(state, version):
    """Build the status panel string from state dict and version."""
    phases = state.get("phases", {})

    # Check if all phases are complete
    all_complete = all(
        phases.get(str(i), {}).get("status") == "complete"
        for i in range(1, 9)
    )

    if all_complete:
        return (
            "============================================\n"
            f"  FLOW — All phases complete!\n"
            f"  Feature: {state['feature']}\n"
            f"  This feature is fully done.\n"
            "============================================"
        )

    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Current Status")
    lines.append("============================================")
    lines.append("")
    lines.append(f"  Feature : {state['feature']}")
    lines.append(f"  Branch  : {state['branch']}")
    lines.append(f"  PR      : {state.get('pr_url', 'N/A')}")
    lines.append("")
    lines.append("  Phases")
    lines.append("  ------")

    current_phase_data = None

    for i in range(1, 9):
        phase = phases.get(str(i), {})
        status = phase.get("status", "pending")
        name = PHASE_NAMES[i]

        if status == "complete":
            marker = "[x]"
            seconds = phase.get("cumulative_seconds", 0)
            time_str = format_time(seconds)
            padded_name = name.ljust(NAME_WIDTH)
            lines.append(f"  {marker} Phase {i}:  {padded_name} ({time_str})")
        elif status == "in_progress":
            marker = "[>]"
            padded_name = name.ljust(NAME_WIDTH)
            lines.append(f"  {marker} Phase {i}:  {padded_name} <-- YOU ARE HERE")
            current_phase_data = phase
        else:
            marker = "[ ]"
            lines.append(f"  {marker} Phase {i}:  {name}")

    lines.append("")

    if current_phase_data:
        seconds = current_phase_data.get("cumulative_seconds", 0)
        visits = current_phase_data.get("visit_count", 0)
        lines.append(f"  Time in current phase : {format_time(seconds)}")
        lines.append(f"  Times visited         : {visits}")
        lines.append("")

    # Find next command
    current = state.get("current_phase", 1)
    current_status = phases.get(str(current), {}).get("status", "pending")
    if current_status == "in_progress":
        cmd = COMMANDS.get(current, f"/flow:phase{current}")
    else:
        cmd = COMMANDS.get(current + 1, COMMANDS.get(current, ""))
    lines.append(f"  Next: {cmd}")
    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def main():
    if len(sys.argv) < 3:
        print(json.dumps({
            "status": "error",
            "message": "Usage: python3 format-status.py <state_file_path> <version>",
        }))
        sys.exit(1)

    state_path = Path(sys.argv[1])
    version = sys.argv[2]

    if not state_path.exists():
        print(json.dumps({"status": "no_state"}))
        sys.exit(0)

    try:
        state = json.loads(state_path.read_text())
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)

    panel = format_panel(state, version)
    print(json.dumps({"status": "ok", "panel": panel}))


if __name__ == "__main__":
    main()
