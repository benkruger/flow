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
    PACIFIC,
    PHASE_NAMES,
    PHASE_NUMBER,
    PHASE_ORDER,
    derive_feature,
    derive_worktree,
    elapsed_since,
    extract_issue_numbers,
    format_time,
    short_issue_ref,
)


def flow_summary(state, now=None):
    """Convert a state dict to a display-ready summary dict."""
    if now is None:
        now = datetime.now(PACIFIC)

    branch = state["branch"]
    current_phase = state.get("current_phase", "flow-start")

    elapsed_seconds = elapsed_since(state.get("started_at"), now)

    issues_filed = state.get("issues_filed", [])
    issues = [
        {
            "label": entry.get("label", ""),
            "title": entry.get("title", ""),
            "url": entry.get("url", ""),
            "ref": short_issue_ref(entry.get("url", "")),
            "phase_name": entry.get("phase_name", ""),
        }
        for entry in issues_filed
    ]

    files = state.get("files", {})
    plan_path = files.get("plan") or state.get("plan_file")

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
        "issues_count": len(issues_filed),
        "issues": issues,
        "blocked": bool(state.get("_blocked")),
        "issue_numbers": set(extract_issue_numbers(state.get("prompt", ""))),
        "plan_path": plan_path,
        "phases": state.get("phases", {}),
        "state": state,
    }


def phase_timeline(state):
    """Build a list of phase display entries from a state dict."""
    phases = state.get("phases", {})
    code_task = state.get("code_task", 0)
    code_tasks_total = state.get("code_tasks_total", 0)
    code_review_step = state.get("code_review_step", 0)
    learn_step = state.get("learn_step", 0)
    complete_step = state.get("complete_step", 0)
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
        if key == "flow-code" and status == "in_progress":
            current_task = code_task + 1
            task_str = f"task {current_task} of {code_tasks_total}" if code_tasks_total > 0 else f"task {current_task}"
            parts = [task_str]
            if diff_stats:
                ins = diff_stats.get("insertions", 0)
                dels = diff_stats.get("deletions", 0)
                parts.append(f"+{ins} -{dels}")
            annotation = ", ".join(parts)
        elif key == "flow-code-review" and status == "in_progress" and code_review_step < 4:
            annotation = f"step {code_review_step + 1} of 4"
        elif key == "flow-learn" and status == "in_progress" and learn_step > 0:
            annotation = f"step {learn_step + 1}"
        elif key == "flow-complete" and status == "in_progress" and complete_step > 0:
            annotation = f"step {complete_step}"

        entries.append(
            {
                "key": key,
                "name": name,
                "number": number,
                "status": status,
                "time": time_str,
                "annotation": annotation,
            }
        )

    return entries


_LOG_LINE_PATTERN = re.compile(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[^\s]*)\s+(.+)$")


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


_STATUS_ICONS = {
    "completed": "\u2713",
    "failed": "\u2717",
    "in_progress": "\u25b6",
    "pending": "\u00b7",
}


def load_orchestration(root):
    """Read .flow-states/orchestrate.json and return the state dict.

    Returns None if the file does not exist, is corrupt, or the state
    directory does not exist.
    """
    root = Path(root)
    state_dir = root / ".flow-states"
    if not state_dir.is_dir():
        return None

    path = state_dir / "orchestrate.json"
    if not path.exists():
        return None

    try:
        return json.loads(path.read_text())
    except (json.JSONDecodeError, ValueError):
        return None


def orchestration_summary(state, now=None):
    """Convert an orchestrate state dict to a display-ready summary.

    Returns None if state is None. Otherwise returns a dict with:
    - elapsed: formatted total elapsed time
    - completed_count, failed_count, total: queue counts
    - is_running: True if completed_at is None
    - items: list of per-item display dicts
    """
    if state is None:
        return None

    if now is None:
        now = datetime.now(PACIFIC)

    started_at = state.get("started_at")
    completed_at = state.get("completed_at")

    if completed_at:
        elapsed_seconds = elapsed_since(started_at, datetime.fromisoformat(completed_at))
    else:
        elapsed_seconds = elapsed_since(started_at, now)

    queue = state.get("queue", [])
    completed_count = sum(1 for item in queue if item.get("outcome") == "completed")
    failed_count = sum(1 for item in queue if item.get("outcome") == "failed")

    items = []
    for item in queue:
        status = item.get("status", "pending")
        icon = _STATUS_ICONS.get(status, "\u00b7")

        item_started = item.get("started_at")
        item_completed = item.get("completed_at")
        if item_completed and item_started:
            item_elapsed = format_time(elapsed_since(item_started, datetime.fromisoformat(item_completed)))
        elif item_started and status == "in_progress":
            item_elapsed = format_time(elapsed_since(item_started, now))
        else:
            item_elapsed = ""

        items.append(
            {
                "icon": icon,
                "issue_number": item.get("issue_number"),
                "title": item.get("title", ""),
                "elapsed": item_elapsed,
                "pr_url": item.get("pr_url"),
                "reason": item.get("reason"),
                "status": status,
            }
        )

    return {
        "elapsed": format_time(elapsed_seconds),
        "completed_count": completed_count,
        "failed_count": failed_count,
        "total": len(queue),
        "is_running": completed_at is None,
        "items": items,
    }
