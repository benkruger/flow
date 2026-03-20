"""Generate morning report from orchestration state.

Usage: bin/flow orchestrate-report --state-file <path> --output-dir <dir>

Output (JSON to stdout):
  Success: {"status": "ok", "summary": "...", "completed": N, "failed": N, "total": N}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import format_time


def _compute_duration_seconds(started_at, completed_at):
    """Compute duration in seconds between two ISO 8601 timestamps."""
    try:
        start = datetime.fromisoformat(started_at)
        end = datetime.fromisoformat(completed_at)
        return max(0, int((end - start).total_seconds()))
    except Exception:
        return 0


def generate_report(state):
    """Generate a morning report from orchestrate state dict.

    Returns dict with summary text, completed count, failed count, and total.
    """
    queue = state.get("queue", [])
    started_at = state.get("started_at", "")
    completed_at = state.get("completed_at", "")

    completed_items = [item for item in queue if item.get("outcome") == "completed"]
    failed_items = [item for item in queue if item.get("outcome") == "failed"]

    duration_seconds = _compute_duration_seconds(started_at, completed_at)
    duration_str = format_time(duration_seconds)

    lines = []
    lines.append("# FLOW Orchestration Report")
    lines.append("")
    lines.append(f"Started: {started_at}")
    lines.append(f"Completed: {completed_at}")
    lines.append(f"Duration: {duration_str}")
    lines.append("")

    if queue:
        lines.append("## Results")
        lines.append("")
        lines.append("| # | Issue | Outcome | PR |")
        lines.append("|---|-------|---------|-----|")
        for i, item in enumerate(queue, 1):
            issue_num = item.get("issue_number", "?")
            title = item.get("title", "")
            outcome = item.get("outcome", "pending")
            pr_url = item.get("pr_url")
            pr_display = pr_url if pr_url else "\u2014"
            lines.append(f"| {i} | #{issue_num} {title} | {outcome} | {pr_display} |")
        lines.append("")

    if completed_items:
        lines.append(f"## Completed ({len(completed_items)})")
        lines.append("")
        for item in completed_items:
            issue_num = item.get("issue_number", "?")
            title = item.get("title", "")
            pr_url = item.get("pr_url", "")
            lines.append(f"- #{issue_num} {title} \u2014 {pr_url}")
        lines.append("")

    if failed_items:
        lines.append(f"## Failed ({len(failed_items)})")
        lines.append("")
        for item in failed_items:
            issue_num = item.get("issue_number", "?")
            title = item.get("title", "")
            reason = item.get("reason", "Unknown")
            lines.append(f"- #{issue_num} {title} \u2014 {reason}")
        lines.append("")

    summary = "\n".join(lines)

    return {
        "summary": summary,
        "completed": len(completed_items),
        "failed": len(failed_items),
        "total": len(queue),
    }


def generate_and_write_report(state_file, output_dir):
    """Read state file, generate report, write summary file.

    Returns dict with status and report data.
    """
    state_path = Path(state_file)
    if not state_path.exists():
        return {"status": "error", "message": f"State file not found: {state_file}"}

    state = json.loads(state_path.read_text())
    result = generate_report(state)

    output_path = Path(output_dir) / "orchestrate-summary.md"
    output_path.write_text(result["summary"])

    return {
        "status": "ok",
        "summary": result["summary"],
        "completed": result["completed"],
        "failed": result["failed"],
        "total": result["total"],
    }


def main():
    parser = argparse.ArgumentParser(description="Generate orchestration morning report")
    parser.add_argument("--state-file", required=True, help="Path to orchestrate.json")
    parser.add_argument("--output-dir", required=True, help="Path to output directory")

    args = parser.parse_args()

    try:
        result = generate_and_write_report(args.state_file, args.output_dir)
        print(json.dumps(result))
    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
