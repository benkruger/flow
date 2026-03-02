"""Shared utilities for FLOW hooks.

Provides common functions used across multiple hook scripts:
- now: current Pacific Time timestamp
- format_time: human-readable time formatting
- project_root: find the main git repo root (works from worktrees)
- current_branch: get the current git branch name
"""

import subprocess
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

PACIFIC = ZoneInfo("America/Los_Angeles")


def now():
    """Return current Pacific Time timestamp in ISO 8601 format."""
    return datetime.now(PACIFIC).isoformat(timespec="seconds")

PHASE_NAMES = {
    1: "Start", 2: "Plan", 3: "Code", 4: "Review",
    5: "Security", 6: "Reflect", 7: "Cleanup",
}


def format_time(seconds):
    """Format seconds into human-readable time.

    Returns "Xh Ym" if >= 3600, "Xm" if >= 60, "<1m" if < 60.
    """
    if not isinstance(seconds, (int, float)):
        try:
            seconds = int(seconds)
        except (ValueError, TypeError):
            return "?"
    if seconds >= 3600:
        hours = seconds // 3600
        minutes = (seconds % 3600) // 60
        return f"{hours}h {minutes}m"
    if seconds >= 60:
        minutes = seconds // 60
        return f"{minutes}m"
    return "<1m"


def project_root():
    """Find the main git repository root.

    Uses `git worktree list --porcelain` to find the root, which works
    correctly whether run from the project root or from inside a worktree.
    Falls back to Path(".") if git fails.
    """
    try:
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            capture_output=True, text=True, check=True,
        )
        for line in result.stdout.strip().split("\n"):
            if line.startswith("worktree "):
                return Path(line.split(" ", 1)[1].strip())
    except Exception:
        pass
    return Path(".")


def current_branch():
    """Get the current git branch name.

    Returns None if not on a branch (e.g. detached HEAD) or if git fails.
    """
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            capture_output=True, text=True, check=True,
        )
        return result.stdout.strip() or None
    except Exception:
        return None
