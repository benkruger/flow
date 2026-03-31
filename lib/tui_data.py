"""Pure data layer for the FLOW interactive TUI.

Reads state files, computes display structs (flow summaries, phase timelines,
log entries). No curses dependency — fully testable with make_state() fixture.

Usage: imported by lib/tui.py
"""

import json
import re
import sys
import time
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

# Static mapping of (phase_key, display_step_number) → short step name.
# Display step number is what the user sees in the annotation.
# Source: skill SKILL.md step headings (## Step N — Name).
STEP_NAMES = {
    "flow-start": {
        3: "creating state",
        4: "labeling issues",
        5: "pulling main",
        6: "running CI",
        7: "updating deps",
        8: "CI after deps",
        9: "committing",
        10: "releasing lock",
        11: "setting up workspace",
    },
    "flow-plan": {
        1: "reading context",
        2: "decomposing",
        3: "writing plan",
        4: "storing plan",
    },
    "flow-code-review": {
        1: "simplifying",
        2: "reviewing",
        3: "security review",
        4: "agent reviews",
    },
    "flow-learn": {
        1: "gathering sources",
        2: "synthesizing",
        3: "applying learnings",
        4: "promoting perms",
        5: "committing",
        6: "filing issues",
        7: "presenting report",
    },
    "flow-complete": {
        1: "checking state",
        2: "checking PR",
        3: "merging main",
        4: "running CI",
        5: "checking GitHub CI",
        6: "confirming merge",
        7: "archiving to PR",
        8: "merging PR",
        9: "closing issues",
        10: "post-merge ops",
        11: "cleaning up",
        12: "pulling changes",
    },
}


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

    timeline = phase_timeline(state, now=now)
    annotation = next((e["annotation"] for e in timeline if e["key"] == current_phase), "")
    phase_elapsed = next(
        (e["time"] for e in timeline if e["key"] == current_phase and e["status"] == "in_progress"), ""
    )

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
        "annotation": annotation,
        "phase_elapsed": phase_elapsed,
        "timeline": timeline,
        "phases": state.get("phases", {}),
        "state": state,
    }


def _step_annotation(step, total=0, name=""):
    """Return 'name - step N of M' or 'step N of M' or '' depending on what's populated."""
    if step <= 0:
        return ""
    step_str = f"step {step} of {total}" if total > 0 else f"step {step}"
    return f"{name} - {step_str}" if name else step_str


def phase_timeline(state, now=None):
    """Build a list of phase display entries from a state dict."""
    if now is None:
        now = datetime.now(PACIFIC)
    phases = state.get("phases", {})
    start_step = state.get("start_step", 0)
    start_steps_total = state.get("start_steps_total", 0)
    plan_step = state.get("plan_step", 0)
    plan_steps_total = state.get("plan_steps_total", 0)
    code_task = state.get("code_task", 0)
    code_tasks_total = state.get("code_tasks_total", 0)
    code_task_name = state.get("code_task_name", "")
    code_review_step = state.get("code_review_step", 0)
    learn_step = state.get("learn_step", 0)
    learn_steps_total = state.get("learn_steps_total", 0)
    complete_step = state.get("complete_step", 0)
    complete_steps_total = state.get("complete_steps_total", 0)
    diff_stats = state.get("diff_stats")
    entries = []

    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        status = phase.get("status", "pending")
        seconds = phase.get("cumulative_seconds", 0)
        number = PHASE_NUMBER[key]
        name = PHASE_NAMES[key]

        if status == "complete":
            time_str = format_time(seconds)
        elif status == "in_progress":
            session_started = phase.get("session_started_at")
            if session_started:
                seconds += elapsed_since(session_started, now)
            time_str = format_time(seconds) if seconds > 0 else ""
        else:
            time_str = ""

        annotation = ""
        if key == "flow-start" and status == "in_progress":
            step_name = STEP_NAMES.get("flow-start", {}).get(start_step, "")
            annotation = _step_annotation(start_step, start_steps_total, step_name)
        elif key == "flow-plan" and status == "in_progress":
            step_name = STEP_NAMES.get("flow-plan", {}).get(plan_step, "")
            annotation = _step_annotation(plan_step, plan_steps_total, step_name)
        elif key == "flow-code" and status == "in_progress":
            current_task = code_task + 1
            if code_tasks_total > 0:
                current_task = min(current_task, code_tasks_total)
            task_str = f"task {current_task} of {code_tasks_total}" if code_tasks_total > 0 else f"task {current_task}"
            if code_task_name:
                truncated = code_task_name[:27] + "..." if len(code_task_name) > 30 else code_task_name
                task_str = f"{truncated} - {task_str}"
            parts = [task_str]
            if diff_stats:
                ins = diff_stats.get("insertions", 0)
                dels = diff_stats.get("deletions", 0)
                parts.append(f"+{ins} -{dels}")
            annotation = ", ".join(parts)
        elif key == "flow-code-review" and status == "in_progress":
            cr_total = len(STEP_NAMES.get("flow-code-review", {}))
            display_step = code_review_step + 1
            if display_step <= cr_total:
                step_name = STEP_NAMES.get("flow-code-review", {}).get(display_step, "")
                annotation = _step_annotation(display_step, cr_total, step_name)
        elif key == "flow-learn" and status == "in_progress":
            display_step = learn_step + 1
            step_name = STEP_NAMES.get("flow-learn", {}).get(display_step, "")
            annotation = _step_annotation(display_step, learn_steps_total, step_name)
        elif key == "flow-complete" and status == "in_progress":
            step_name = STEP_NAMES.get("flow-complete", {}).get(complete_step, "")
            annotation = _step_annotation(complete_step, complete_steps_total, step_name)

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


_STALE_THRESHOLD_SECONDS = 600  # 10 minutes


def load_account_metrics(repo_root):
    """Load account metrics (monthly cost, rate limits) for TUI header display.

    Returns dict with keys: cost_monthly (str), rl_5h (int|None),
    rl_7d (int|None), stale (bool).
    """
    repo_root = Path(repo_root)

    # --- Monthly cost from per-session cost files ---
    year_month = time.strftime("%Y-%m")
    cost_dir = repo_root / ".claude" / "cost" / year_month
    total_cost = 0.0
    if cost_dir.is_dir():
        for cost_file in cost_dir.iterdir():
            try:
                total_cost += float(cost_file.read_text().strip())
            except (ValueError, OSError):
                continue
    cost_monthly = f"{total_cost:.2f}"

    # --- Rate limits from ~/.claude/rate-limits.json ---
    rl_path = Path.home() / ".claude" / "rate-limits.json"
    rl_5h = None
    rl_7d = None
    stale = True

    try:
        mtime = rl_path.stat().st_mtime
        age = time.time() - mtime
        if age <= _STALE_THRESHOLD_SECONDS:
            data = json.loads(rl_path.read_text())
            rl_5h = int(data["five_hour_pct"])
            rl_7d = int(data["seven_day_pct"])
            stale = False
    except (OSError, json.JSONDecodeError, KeyError, ValueError, TypeError):
        pass

    return {
        "cost_monthly": cost_monthly,
        "rl_5h": rl_5h,
        "rl_7d": rl_7d,
        "stale": stale,
    }
