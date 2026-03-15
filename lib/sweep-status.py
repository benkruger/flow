"""Format the FLOW sweep dashboard.

Usage: bin/flow sweep-status

Reads .flow-states/sweep.json and formats a dashboard table showing
all issues being processed, their status, and PR links.

Output:
  Exit 0: stdout = dashboard text
  Exit 1: no sweep.json found
  Exit 2: stderr = error message
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PACIFIC, project_root


def _read_version():
    """Read plugin version from plugin.json next to this script."""
    plugin_json = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    try:
        return json.loads(plugin_json.read_text())["version"]
    except Exception:
        return "?"


def format_dashboard(sweep, version):
    """Build the sweep dashboard string from sweep state dict."""
    issues = sweep.get("issues", [])

    counts = {"queued": 0, "in_progress": 0, "complete": 0, "failed": 0}
    for issue in issues:
        status = issue.get("status", "queued")
        counts[status] = counts.get(status, 0) + 1

    total = len(issues)
    summary_parts = []
    for status in ("complete", "in_progress", "queued", "failed"):
        if counts.get(status, 0) > 0:
            summary_parts.append(f"{counts[status]} {status.replace('_', ' ')}")

    lines = []
    lines.append("============================================")
    lines.append(f"  FLOW v{version} — Sweep Status")
    lines.append("============================================")
    lines.append("")
    lines.append(f"  Issues: {total} total — {', '.join(summary_parts)}")
    lines.append("")

    if issues:
        lines.append("  #   | Title                          | Status      | PR")
        lines.append("  ----|--------------------------------|-------------|--------")
        for issue in issues:
            number = str(issue.get("number", "?")).rjust(3)
            title = issue.get("title", "")[:30].ljust(30)
            status = issue.get("status", "queued").replace("_", " ").ljust(11)
            pr_url = issue.get("pr_url")
            pr_display = f"#{issue['pr_number']}" if issue.get("pr_number") else "—"
            lines.append(f"  {number} | {title} | {status} | {pr_display}")
        lines.append("")

    lines.append("============================================")
    return "\n".join(lines)


def main():
    root = project_root()
    sweep_path = root / ".flow-states" / "sweep.json"

    if not sweep_path.exists():
        sys.exit(1)

    try:
        sweep = json.loads(sweep_path.read_text())
    except Exception as e:
        print(f"Error reading sweep.json: {e}", file=sys.stderr)
        sys.exit(2)

    version = _read_version()
    print(format_dashboard(sweep, version))


if __name__ == "__main__":
    main()
