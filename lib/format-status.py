"""Format the FLOW status panel.

Usage: bin/flow format-status

Derives state file path (via git) and plugin version (via plugin.json)
internally — no arguments needed.

Output:
  Exit 0: stdout = panel text (single feature or multiple features)
  Exit 1: no stdout (no state file found)
  Exit 2: stderr = error message
"""

import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    current_branch, find_state_files, format_time, PACIFIC,
    project_root, COMMANDS, PHASE_NAMES, PHASE_NUMBER, PHASE_ORDER,
)

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
        phases.get(key, {}).get("status") == "complete"
        for key in PHASE_ORDER
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

    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        status = phase.get("status", "pending")
        name = PHASE_NAMES[key]
        num = PHASE_NUMBER[key]

        if status == "complete":
            marker = "[x]"
            seconds = phase.get("cumulative_seconds", 0)
            time_str = format_time(seconds)
            padded_name = name.ljust(NAME_WIDTH)
            lines.append(f"  {marker} Phase {num}:  {padded_name} ({time_str})")
        elif status == "in_progress":
            marker = "[>]"
            padded_name = name.ljust(NAME_WIDTH)
            lines.append(f"  {marker} Phase {num}:  {padded_name} <-- YOU ARE HERE")
            current_phase_data = phase
        else:
            marker = "[ ]"
            lines.append(f"  {marker} Phase {num}:  {name}")

    lines.append("")

    if current_phase_data:
        seconds = current_phase_data.get("cumulative_seconds", 0)
        visits = current_phase_data.get("visit_count", 0)
        lines.append(f"  Time in current phase : {format_time(seconds)}")
        lines.append(f"  Times visited         : {visits}")
        lines.append("")

    # Continue (in_progress) vs Next (pending)
    # current_phase already points to the next phase after phase-transition
    # --action complete, so COMMANDS[current] is always the right command.
    current = state.get("current_phase", "flow-start")
    current_status = phases.get(current, {}).get("status", "pending")
    if current_status == "in_progress":
        cmd = COMMANDS.get(current, f"/flow:{current}")
        lines.append(f"  Continue: {cmd}")
    else:
        cmd = COMMANDS.get(current, "")
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
        phases.get(key, {}).get("cumulative_seconds", 0)
        for key in PHASE_ORDER
    )
    lines.append(f"  Elapsed : {format_time(total)}")

    lines.append("")
    lines.append("  Phases")
    lines.append("  ------")

    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        padded_name = PHASE_NAMES[key].ljust(NAME_WIDTH)
        seconds = phase.get("cumulative_seconds", 0)
        time_str = format_time(seconds)
        num = PHASE_NUMBER[key]
        lines.append(f"  [x] Phase {num}:  {padded_name} ({time_str})")

    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def format_multi_panel(results, version):
    """Build a summary panel listing multiple active features."""
    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Multiple Features Active")
    lines.append("============================================")
    lines.append("")

    for i, (path, state, matched_branch) in enumerate(results, 1):
        phase_key = state.get("current_phase", "flow-start")
        phase_name = PHASE_NAMES.get(phase_key, phase_key)
        phase_num = PHASE_NUMBER.get(phase_key, "?")
        phase_status = state.get("phases", {}).get(
            phase_key, {},
        ).get("status", "pending")
        cmd = COMMANDS.get(phase_key, f"/flow:{phase_key}")
        lines.append(f"  {i}. {state.get('feature', matched_branch)}")
        lines.append(f"     Branch : {matched_branch}")
        lines.append(f"     Phase  : {phase_num} — {phase_name} ({phase_status})")
        lines.append(f"     Next   : {cmd}")
        lines.append("")

    lines.append("============================================")
    return "\n".join(lines)


def main():
    root = project_root()
    branch = current_branch()

    if not branch:
        print("Could not determine current branch", file=sys.stderr)
        sys.exit(2)

    results = find_state_files(root, branch)

    if not results:
        sys.exit(1)

    version = _read_version()

    if len(results) > 1:
        panel = format_multi_panel(results, version)
        print(panel)
        sys.exit(0)

    state_path, state, matched_branch = results[0]
    panel = format_panel(state, version)
    print(panel)


if __name__ == "__main__":
    main()
