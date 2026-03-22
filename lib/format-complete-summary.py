"""Format the Complete phase Done banner as a business-friendly summary.

Usage: bin/flow format-complete-summary --state-file <path> [--closed-issues-file <path>]

Output (JSON to stdout):
  Success: {"status": "ok", "summary": "...", "total_seconds": N}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    derive_feature, format_time, read_version, short_issue_ref,
    PHASE_NAMES, PHASE_ORDER,
)

MAX_PROMPT_LENGTH = 80


def _truncate_prompt(prompt):
    """Truncate prompt to MAX_PROMPT_LENGTH chars with ellipsis if needed."""
    if len(prompt) <= MAX_PROMPT_LENGTH:
        return prompt
    return prompt[:MAX_PROMPT_LENGTH] + "..."


def format_complete_summary(state, closed_issues=None):
    """Build the Complete phase Done banner from state dict.

    Args:
        state: The state file dict.
        closed_issues: Optional list of closed issue dicts with 'number'
            and optional 'url' keys.

    Returns dict with summary (str) and total_seconds (int).
    """
    branch = state.get("branch", "unknown")
    feature = derive_feature(branch)
    prompt = state.get("prompt", "")
    pr_url = state.get("pr_url", "N/A")
    phases = state.get("phases", {})
    issues = state.get("issues_filed", [])
    notes = state.get("notes", [])
    version = read_version()

    # Build phase timing rows and total
    total_seconds = 0
    timing_lines = []
    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        seconds = phase.get("cumulative_seconds", 0)
        total_seconds += seconds
        name = PHASE_NAMES.get(key, key)
        timing_lines.append(f"  {name + ':':<16} {format_time(seconds)}")

    # Build the summary
    border = "━" * 58
    lines = []
    lines.append(border)
    lines.append(f"  ✓ FLOW v{version} — Complete")
    lines.append(border)
    lines.append("")
    lines.append(f"  Feature:  {feature}")
    lines.append(f"  What:     {_truncate_prompt(prompt)}")
    lines.append(f"  PR:       {pr_url}")

    # Resolved section (closed issues from close-issues)
    if closed_issues:
        lines.append("")
        lines.append("  Resolved")
        lines.append("  " + "─" * 28)
        for resolved in closed_issues:
            num = resolved["number"]
            url = resolved.get("url", "")
            if url:
                lines.append(f"    #{num} {url}")
            else:
                lines.append(f"    #{num}")

    lines.append("")
    lines.append("  Timeline")
    lines.append("  " + "─" * 28)
    for timing_line in timing_lines:
        lines.append(timing_line)
    lines.append("  " + "─" * 28)
    lines.append(f"  {'Total:':<16} {format_time(total_seconds)}")
    lines.append("")

    # Artifacts section (only if there are issues or notes)
    has_artifacts = len(issues) > 0 or len(notes) > 0
    if has_artifacts:
        lines.append("  Artifacts")
        lines.append("  " + "─" * 28)
        if issues:
            lines.append(f"  Issues filed: {len(issues)}")
            for issue in issues:
                url = issue.get("url", "")
                shorthand = short_issue_ref(url) if url else ""
                prefix = f"{shorthand} " if shorthand.startswith("#") else ""
                lines.append(f"    [{issue['label']}] {prefix}{issue['title']}")
                if url:
                    lines.append(f"    {url}")
        if notes:
            lines.append(f"  Notes captured: {len(notes)}")
        lines.append("")

    lines.append(border)

    summary = "\n".join(lines)
    return {"summary": summary, "total_seconds": total_seconds}


def main():
    parser = argparse.ArgumentParser(description="Format Complete phase summary")
    parser.add_argument("--state-file", required=True, help="Path to state JSON file")
    parser.add_argument("--closed-issues-file", default=None, help="Path to closed issues JSON file")

    args = parser.parse_args()

    try:
        state_path = Path(args.state_file)
        if not state_path.exists():
            print(json.dumps({"status": "error", "message": f"State file not found: {args.state_file}"}))
            return

        state = json.loads(state_path.read_text())

        closed_issues = None
        if args.closed_issues_file:
            closed_path = Path(args.closed_issues_file)
            if closed_path.exists():
                closed_issues = json.loads(closed_path.read_text())

        result = format_complete_summary(state, closed_issues=closed_issues)

        print(json.dumps({
            "status": "ok",
            "summary": result["summary"],
            "total_seconds": result["total_seconds"],
        }))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
