"""Pure data layer for the FLOW interactive TUI.

Reads state files, computes display structs (flow summaries, phase timelines,
log entries). No curses dependency — fully testable with make_state() fixture.

Usage: imported by lib/tui.py
"""

import json
import re
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    PACIFIC, PHASE_NAMES, PHASE_NUMBER, PHASE_ORDER,
    derive_feature, derive_worktree, elapsed_since, format_time,
)


def flow_summary(state, now=None):
    """Convert a state dict to a display-ready summary dict."""
    if now is None:
        now = datetime.now(PACIFIC)

    branch = state["branch"]
    current_phase = state.get("current_phase", "flow-start")

    elapsed_seconds = elapsed_since(state.get("started_at"), now)

    return {
        "feature": derive_feature(branch),
        "branch": branch,
        "worktree": derive_worktree(branch),
        "pr_number": state.get("pr_number"),
        "pr_url": state.get("pr_url"),
        "phase_number": PHASE_NUMBER.get(current_phase, 0),
        "phase_name": PHASE_NAMES.get(current_phase, current_phase),
        "elapsed": format_time(elapsed_seconds),
        "code_task": state.get("code_task", 0),
        "diff_stats": state.get("diff_stats"),
        "notes_count": len(state.get("notes", [])),
        "issues_count": len(state.get("issues_filed", [])),
        "phases": state.get("phases", {}),
        "state": state,
    }


def phase_timeline(state):
    """Build a list of phase display entries from a state dict."""
    phases = state.get("phases", {})
    code_task = state.get("code_task", 0)
    diff_stats = state.get("diff_stats")
    entries = []

    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        status = phase.get("status", "pending")
        seconds = phase.get("cumulative_seconds", 0)
        number = PHASE_NUMBER[key]
        name = PHASE_NAMES[key]

        time_str = format_time(seconds) if status == "complete" else ""

        annotation = ""
        if key == "flow-code" and status == "in_progress" and code_task > 0:
            parts = [f"task {code_task}"]
            if diff_stats:
                ins = diff_stats.get("insertions", 0)
                dels = diff_stats.get("deletions", 0)
                parts.append(f"+{ins} -{dels}")
            annotation = ", ".join(parts)

        entries.append({
            "key": key,
            "name": name,
            "number": number,
            "status": status,
            "time": time_str,
            "annotation": annotation,
        })

    return entries


_LOG_LINE_PATTERN = re.compile(
    r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[^\s]*)\s+(.+)$"
)


def parse_log_entries(log_content, limit=20):
    """Parse log file content into display entries.

    Each log line has format: <ISO8601-Pacific> <message>
    Returns last `limit` entries as {"time": "HH:MM", "message": "..."} dicts.
    """
    if not log_content:
        return []

    entries = []
    for line in log_content.strip().split("\n"):
        line = line.strip()
        if not line:
            continue
        match = _LOG_LINE_PATTERN.match(line)
        if not match:
            continue
        timestamp_str = match.group(1)
        message = match.group(2)
        try:
            parsed = datetime.fromisoformat(timestamp_str)
            time_display = parsed.strftime("%H:%M")
        except ValueError:
            continue
        entries.append({"time": time_display, "message": message})

    return entries[-limit:]


def load_all_flows(root):
    """Read all .flow-states/*.json state files and return flow summaries.

    Returns a list of flow_summary dicts sorted by feature name.
    Skips corrupt JSON and non-state files (e.g., *-phases.json).
    """
    root = Path(root)
    state_dir = root / ".flow-states"
    if not state_dir.is_dir():
        return []

    flows = []
    for path in sorted(state_dir.glob("*.json")):
        if path.name.endswith("-phases.json"):
            continue
        try:
            state = json.loads(path.read_text())
            if "branch" not in state:
                continue
            flows.append(flow_summary(state))
        except (json.JSONDecodeError, ValueError, KeyError):
            continue

    flows.sort(key=lambda f: f["feature"])
    return flows
