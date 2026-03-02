"""Format the FLOW status panel.

Usage: bin/flow format-status

Derives state file path (via git) and plugin version (via plugin.json)
internally — no arguments needed.

Output (JSON to stdout):
  Success: {"status": "ok", "panel": "..."}
  No state: {"status": "no_state"}
  Error:   {"status": "error", "message": "..."}
"""

import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import current_branch, format_time, PACIFIC, project_root, PHASE_NAMES

COMMANDS = {
    1: "/flow:start", 2: "/flow:plan", 3: "/flow:code",
    4: "/flow:review", 5: "/flow:security", 6: "/flow:reflect",
    7: "/flow:cleanup",
}

# Column width for phase name alignment
NAME_WIDTH = 12


def _elapsed_since(started_at, now=None):
    """Calculate elapsed seconds from an ISO timestamp to now."""
    if not started_at:
        return 0
    if now is None:
        now = datetime.now(PACIFIC)
    start = datetime.fromisoformat(started_at)
    return max(0, int((now - start).total_seconds()))


def _read_version():
    """Read plugin version from plugin.json next to this script."""
    plugin_json = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    try:
        return json.loads(plugin_json.read_text())["version"]
    except Exception:
        return "?"


def format_panel(state, version, now=None):
    """Build the status panel string from state dict and version."""
    phases = state.get("phases", {})

    # Check if all phases are complete
    all_complete = all(
        phases.get(str(i), {}).get("status") == "complete"
        for i in range(1, 8)
    )

    if all_complete:
        return _format_all_complete(state, version, phases)

    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Current Status")
    lines.append("============================================")
    lines.append("")
    lines.append(f"  Feature : {state['feature']}")
    lines.append(f"  Branch  : {state['branch']}")
    lines.append(f"  PR      : {state.get('pr_url', 'N/A')}")

    # Elapsed time
    elapsed = _elapsed_since(state.get("started_at"), now)
    lines.append(f"  Elapsed : {format_time(elapsed)}")

    # Notes count (omit if zero)
    notes = state.get("notes", [])
    if notes:
        lines.append(f"  Notes   : {len(notes)}")

    lines.append("")
    lines.append("  Phases")
    lines.append("  ------")

    current_phase_data = None

    for i in range(1, 8):
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

    # Continue (in_progress) vs Next (phase complete)
    current = state.get("current_phase", 1)
    current_status = phases.get(str(current), {}).get("status", "pending")
    if current_status == "in_progress":
        cmd = COMMANDS.get(current, f"/flow:phase{current}")
        lines.append(f"  Continue: {cmd}")
    else:
        cmd = COMMANDS.get(current + 1, COMMANDS.get(current, ""))
        lines.append(f"  Next: {cmd}")
    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def _format_all_complete(state, version, phases):
    """Build the enriched all-complete panel."""
    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — All Phases Complete!")
    lines.append("============================================")
    lines.append("")
    lines.append(f"  Feature : {state['feature']}")
    lines.append(f"  PR      : {state.get('pr_url', 'N/A')}")

    # Total elapsed from phase timings
    total = sum(
        phases.get(str(i), {}).get("cumulative_seconds", 0)
        for i in range(1, 8)
    )
    lines.append(f"  Elapsed : {format_time(total)}")

    lines.append("")
    lines.append("  Phases")
    lines.append("  ------")

    for i in range(1, 8):
        phase = phases.get(str(i), {})
        padded_name = PHASE_NAMES[i].ljust(NAME_WIDTH)
        seconds = phase.get("cumulative_seconds", 0)
        time_str = format_time(seconds)
        lines.append(f"  [x] Phase {i}:  {padded_name} ({time_str})")

    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def main():
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

    version = _read_version()

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
