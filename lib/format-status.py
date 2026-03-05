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

from flow_utils import (
    current_branch, find_state_files, format_time, PACIFIC,
    project_root, PHASE_NAMES,
)

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


def format_panel(state, version, now=None, dev_mode=False):
    """Build the status panel string from state dict and version."""
    phases = state.get("phases", {})

    # Check if all phases are complete
    all_complete = all(
        phases.get(str(i), {}).get("status") == "complete"
        for i in range(1, 9)
    )

    if all_complete:
        return _format_all_complete(state, version, phases, dev_mode=dev_mode)

    dev_label = " [DEV MODE]" if dev_mode else ""
    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Current Status{dev_label}")
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

    # Continue (in_progress) vs Next (pending)
    # current_phase already points to the next phase after phase-transition
    # --action complete, so COMMANDS[current] is always the right command.
    current = state.get("current_phase", 1)
    current_status = phases.get(str(current), {}).get("status", "pending")
    if current_status == "in_progress":
        cmd = COMMANDS.get(current, f"/flow:phase{current}")
        lines.append(f"  Continue: {cmd}")
    else:
        cmd = COMMANDS.get(current, "")
        lines.append(f"  Next: {cmd}")
    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def _format_all_complete(state, version, phases, dev_mode=False):
    """Build the enriched all-complete panel."""
    dev_label = " [DEV MODE]" if dev_mode else ""
    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — All Phases Complete!{dev_label}")
    lines.append("============================================")
    lines.append("")
    lines.append(f"  Feature : {state['feature']}")
    lines.append(f"  PR      : {state.get('pr_url', 'N/A')}")

    # Total elapsed from phase timings
    total = sum(
        phases.get(str(i), {}).get("cumulative_seconds", 0)
        for i in range(1, 9)
    )
    lines.append(f"  Elapsed : {format_time(total)}")

    lines.append("")
    lines.append("  Phases")
    lines.append("  ------")

    for i in range(1, 9):
        phase = phases.get(str(i), {})
        padded_name = PHASE_NAMES[i].ljust(NAME_WIDTH)
        seconds = phase.get("cumulative_seconds", 0)
        time_str = format_time(seconds)
        lines.append(f"  [x] Phase {i}:  {padded_name} ({time_str})")

    lines.append("")
    lines.append("============================================")

    return "\n".join(lines)


def format_multi_panel(results, version, dev_mode=False):
    """Build a summary panel listing multiple active features."""
    dev_label = " [DEV MODE]" if dev_mode else ""
    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Multiple Features Active{dev_label}")
    lines.append("============================================")
    lines.append("")

    for i, (path, state, matched_branch) in enumerate(results, 1):
        phase = state.get("current_phase", 1)
        phase_name = PHASE_NAMES.get(phase, f"Phase {phase}")
        phase_status = state.get("phases", {}).get(
            str(phase), {},
        ).get("status", "pending")
        cmd = COMMANDS.get(phase, f"/flow:phase{phase}")
        lines.append(f"  {i}. {state.get('feature', matched_branch)}")
        lines.append(f"     Branch : {matched_branch}")
        lines.append(f"     Phase  : {phase} — {phase_name} ({phase_status})")
        lines.append(f"     Next   : {cmd}")
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

    results = find_state_files(root, branch)

    if not results:
        print(json.dumps({"status": "no_state"}))
        sys.exit(0)

    version = _read_version()
    dev_mode = (root / ".flow-states" / ".dev-mode").exists()

    if len(results) > 1:
        panel = format_multi_panel(results, version, dev_mode=dev_mode)
        print(json.dumps({
            "status": "multiple_features",
            "panel": panel,
        }))
        sys.exit(0)

    state_path, state, matched_branch = results[0]
    panel = format_panel(state, version, dev_mode=dev_mode)
    print(json.dumps({"status": "ok", "panel": panel}))


if __name__ == "__main__":
    main()
