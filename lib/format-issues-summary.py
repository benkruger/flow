"""Format issues summary for the Complete phase.

Usage: bin/flow format-issues-summary --state-file <path> --output <path>

Reads issues_filed from the state file, formats a markdown table and a
banner summary line. Writes the table to --output if issues exist.

Output (JSON to stdout):
  Success: {"status": "ok", "has_issues": true/false, "banner_line": "...", "table": "..."}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from collections import OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import short_issue_ref


def format_issues_summary(state):
    """Build issues summary from state dict.

    Returns dict with has_issues, banner_line, and table keys.
    """
    issues = state.get("issues_filed", [])

    if not issues:
        return {"has_issues": False, "banner_line": "", "table": ""}

    # Build banner line with label counts in encounter order
    label_counts = OrderedDict()
    for issue in issues:
        label = issue["label"]
        label_counts[label] = label_counts.get(label, 0) + 1

    total = len(issues)
    parts = [f"{label}: {count}" for label, count in label_counts.items()]
    banner_line = f"Issues filed: {total} ({', '.join(parts)})"

    # Build markdown table
    lines = [
        "| Label | Title | Phase | URL |",
        "|-------|-------|-------|-----|",
    ]
    for issue in issues:
        url = issue["url"]
        short_url = short_issue_ref(url)
        lines.append(
            f"| {issue['label']} | {issue['title']} "
            f"| {issue.get('phase_name', issue.get('phase', ''))} "
            f"| {short_url} |"
        )

    table = "\n".join(lines)

    return {"has_issues": True, "banner_line": banner_line, "table": table}


def main():
    parser = argparse.ArgumentParser(description="Format issues summary")
    parser.add_argument("--state-file", required=True, help="Path to state JSON file")
    parser.add_argument("--output", required=True, help="Path to write markdown table")

    args = parser.parse_args()

    try:
        state_path = Path(args.state_file)
        if not state_path.exists():
            print(json.dumps({"status": "error", "message": f"State file not found: {args.state_file}"}))
            return

        state = json.loads(state_path.read_text())
        result = format_issues_summary(state)

        if result["has_issues"]:
            output_path = Path(args.output)
            output_path.parent.mkdir(parents=True, exist_ok=True)
            output_path.write_text(result["table"])

        print(json.dumps({
            "status": "ok",
            "has_issues": result["has_issues"],
            "banner_line": result["banner_line"],
            "table": result["table"],
        }))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
