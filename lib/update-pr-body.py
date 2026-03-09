"""Update PR body with artifact pointers and collapsible sections.

Two modes:
  --add-artifact: Add a labeled line to a ## Artifacts section
  --append-section: Append a collapsible <details> block

Usage:
  bin/flow update-pr-body --pr <N> --add-artifact --label "Plan file" --value "<path>"
  bin/flow update-pr-body --pr <N> --append-section --heading "State File" --summary "<name>" --content-file <path> --format json

Output (JSON to stdout):
  Success: {"status": "ok", "action": "add_artifact|append_section"}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


def _build_artifact_line(label, value):
    """Build a markdown artifact line: - **Label**: `value`."""
    return f"- **{label}**: `{value}`"


def _ensure_artifacts_section(body):
    """Insert ## Artifacts section after ## What paragraph if not present."""
    if "## Artifacts" in body:
        return body

    match = re.search(r"(## What\n\n[^\n]+)", body)
    if match:
        insert_at = match.end()
        return body[:insert_at] + "\n\n## Artifacts\n" + body[insert_at:]

    return body + "\n\n## Artifacts\n"


def _add_artifact_to_body(body, label, value):
    """Add or replace an artifact line in the ## Artifacts section."""
    body = _ensure_artifacts_section(body)
    new_line = _build_artifact_line(label, value)

    pattern = re.compile(rf"^- \*\*{re.escape(label)}\*\*:.*$", re.MULTILINE)
    if pattern.search(body):
        return pattern.sub(new_line, body)

    artifacts_idx = body.index("## Artifacts")
    section_end = body.find("\n## ", artifacts_idx + 1)
    if section_end == -1:
        section_end = len(body)

    insert_at = section_end
    body_before = body[:insert_at].rstrip("\n")
    body_after = body[insert_at:]
    return body_before + "\n\n" + new_line + body_after


def _build_details_block(heading, summary, content, fmt):
    """Build a collapsible details block with heading and fenced code."""
    return (
        f"## {heading}\n\n"
        f"<details>\n"
        f"<summary>{summary}</summary>\n\n"
        f"```{fmt}\n"
        f"{content}\n"
        f"```\n\n"
        f"</details>"
    )


def _append_section_to_body(body, heading, summary, content, fmt):
    """Append or replace a collapsible section in the body."""
    block = _build_details_block(heading, summary, content, fmt)

    pattern = re.compile(
        rf"## {re.escape(heading)}\n\n<details>.*?</details>",
        re.DOTALL,
    )
    if pattern.search(body):
        return pattern.sub(block, body)

    return body.rstrip("\n") + "\n\n" + block


def _gh_get_body(pr_number):
    """Read current PR body via gh."""
    result = subprocess.run(
        ["gh", "pr", "view", str(pr_number), "--json", "body", "--jq", ".body"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    return result.stdout.rstrip("\n")


def _gh_set_body(pr_number, body):
    """Write PR body via gh."""
    result = subprocess.run(
        ["gh", "pr", "edit", str(pr_number), "--body", body],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())


def main():
    parser = argparse.ArgumentParser(description="Update PR body with artifacts")
    parser.add_argument("--pr", type=int, required=True, help="PR number")

    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--add-artifact", action="store_true",
                      help="Add artifact line to ## Artifacts section")
    mode.add_argument("--append-section", action="store_true",
                      help="Append collapsible details section")

    parser.add_argument("--label", help="Artifact label (for --add-artifact)")
    parser.add_argument("--value", help="Artifact value (for --add-artifact)")
    parser.add_argument("--heading", help="Section heading (for --append-section)")
    parser.add_argument("--summary", help="Details summary (for --append-section)")
    parser.add_argument("--content-file", help="Path to content file (for --append-section)")
    parser.add_argument("--format", default="text", help="Code block format (for --append-section)")

    args = parser.parse_args()

    try:
        if args.add_artifact:
            body = _gh_get_body(args.pr)
            new_body = _add_artifact_to_body(body, args.label, args.value)
            _gh_set_body(args.pr, new_body)
            print(json.dumps({"status": "ok", "action": "add_artifact"}))
        else:
            content_path = args.content_file
            if not content_path:
                print(json.dumps({"status": "error", "message": "Missing --content-file"}))
                return

            path = Path(content_path)
            if not path.exists():
                print(json.dumps({"status": "error", "message": f"File not found: {content_path}"}))
                return

            content = path.read_text()
            body = _gh_get_body(args.pr)
            new_body = _append_section_to_body(body, args.heading, args.summary, content, args.format)
            _gh_set_body(args.pr, new_body)
            print(json.dumps({"status": "ok", "action": "append_section"}))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
