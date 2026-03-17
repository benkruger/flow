"""Render the complete PR body from the state file and artifact files.

Produces a consistent, idempotent PR body every time. Called by phase skills
after state updates — replaces piecemeal update-pr-body calls.

Usage:
  bin/flow render-pr-body --pr <N>
  bin/flow render-pr-body --pr <N> --state-file <path> --dry-run

Output (JSON to stdout):
  Success: {"status": "ok", "sections": ["What", "Artifacts", ...]}
  Failure: {"status": "error", "message": "..."}
"""

import argparse
import importlib.util
import json
import re
import subprocess
import sys
from collections import OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PHASE_NAMES, PHASE_ORDER, format_time, project_root, current_branch


def _load_sibling(name, filename):
    """Import a hyphenated sibling module via importlib."""
    lib_dir = Path(__file__).resolve().parent
    spec = importlib.util.spec_from_file_location(name, lib_dir / filename)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


_timings_mod = _load_sibling("format_pr_timings", "format-pr-timings.py")
_issues_mod = _load_sibling("format_issues_summary", "format-issues-summary.py")


def _build_artifacts(state):
    """Build the ## Artifacts section from state fields."""
    items = []
    plan_file = state.get("plan_file")
    if plan_file:
        items.append(f"- **Plan file**: `{plan_file}`")
    dag_file = state.get("dag_file")
    if dag_file:
        items.append(f"- **DAG file**: `{dag_file}`")
    transcript = state.get("transcript_path")
    if transcript:
        items.append(f"- **Session log**: `{transcript}`")
    return items


def _build_details(heading, summary, content, fmt):
    """Build a collapsible <details> section."""
    return (
        f"## {heading}\n\n"
        f"<details>\n"
        f"<summary>{summary}</summary>\n\n"
        f"```{fmt}\n"
        f"{content}\n"
        f"```\n\n"
        f"</details>"
    )


def _build_plain_section(heading, content):
    """Build a plain section with end sentinel."""
    return f"## {heading}\n\n{content}\n\n<!-- end:{heading} -->"


def _format_timings_started_only(state):
    """Format phase timings table showing only phases that have started."""
    phases = state.get("phases", {})
    lines = [
        "| Phase | Duration |",
        "|-------|----------|",
    ]

    total_seconds = 0
    for key in PHASE_ORDER:
        phase = phases.get(key, {})
        started = phase.get("started_at")
        seconds = phase.get("cumulative_seconds", 0)
        if not started and seconds == 0:
            continue
        name = PHASE_NAMES.get(key, key)
        total_seconds += seconds
        lines.append(f"| {name} | {format_time(seconds)} |")

    lines.append(f"| **Total** | **{format_time(total_seconds)}** |")
    return "\n".join(lines)


def _format_issues_table(state):
    """Format issues summary using the existing module."""
    result = _issues_mod.format_issues_summary(state)
    if result["has_issues"]:
        return result["table"]
    return None


def render_body(state, project_dir):
    """Render the complete PR body from state and artifact files.

    Args:
        state: The state dict (already parsed JSON).
        project_dir: Path to the project root (for resolving relative paths).

    Returns:
        The complete PR body as a string.
    """
    sections = []
    section_names = []

    # 1. What (always)
    feature = state.get("feature", "Unknown")
    sections.append(f"## What\n\n{feature}.")
    section_names.append("What")

    # 2. Artifacts (always, items conditional)
    artifact_items = _build_artifacts(state)
    if artifact_items:
        sections.append("## Artifacts\n\n" + "\n\n".join(artifact_items))
    else:
        sections.append("## Artifacts")
    section_names.append("Artifacts")

    # 3. Plan (conditional)
    plan_file = state.get("plan_file")
    if plan_file and Path(plan_file).exists():
        content = Path(plan_file).read_text().rstrip("\n")
        sections.append(_build_details("Plan", "Implementation plan", content, "text"))
        section_names.append("Plan")

    # 4. DAG Analysis (conditional, always text format)
    dag_file = state.get("dag_file")
    if dag_file and Path(dag_file).exists():
        content = Path(dag_file).read_text().rstrip("\n")
        sections.append(
            _build_details("DAG Analysis", "Decompose plugin output", content, "text")
        )
        section_names.append("DAG Analysis")

    # 5. Phase Timings (always, started phases only)
    timings_table = _format_timings_started_only(state)
    sections.append(_build_plain_section("Phase Timings", timings_table))
    section_names.append("Phase Timings")

    # 6. State File (always)
    state_json = json.dumps(state, indent=2)
    branch = state.get("branch", "unknown")
    sections.append(
        _build_details("State File", f".flow-states/{branch}.json", state_json, "json")
    )
    section_names.append("State File")

    # 7. Session Log (conditional)
    branch = state.get("branch", "unknown")
    log_path = Path(project_dir) / ".flow-states" / f"{branch}.log"
    if log_path.exists():
        content = log_path.read_text().rstrip("\n")
        sections.append(
            _build_details(
                "Session Log", f".flow-states/{branch}.log", content, "text"
            )
        )
        section_names.append("Session Log")

    # 8. Issues Filed (conditional)
    issues_table = _format_issues_table(state)
    if issues_table:
        sections.append(_build_plain_section("Issues Filed", issues_table))
        section_names.append("Issues Filed")

    return "\n\n".join(sections)


def _gh_set_body(pr_number, body):
    """Write PR body via gh."""
    result = subprocess.run(
        ["gh", "pr", "edit", str(pr_number), "--body", body],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())


def main():
    parser = argparse.ArgumentParser(description="Render complete PR body from state")
    parser.add_argument("--pr", type=int, required=True, help="PR number")
    parser.add_argument("--state-file", help="Path to state file (auto-detected if omitted)")
    parser.add_argument("--dry-run", action="store_true",
                        help="Generate body and return sections without updating PR")

    args = parser.parse_args()

    try:
        if args.state_file:
            state_path = Path(args.state_file)
        else:
            root = project_root()
            branch = current_branch()
            state_path = Path(root) / ".flow-states" / f"{branch}.json"

        if not state_path.exists():
            print(json.dumps({"status": "error", "message": f"State file not found: {state_path}"}))
            return

        state = json.loads(state_path.read_text())
        project_dir = state_path.parent.parent if not args.state_file else state_path.parent.parent

        body = render_body(state, project_dir)

        if not args.dry_run:
            _gh_set_body(args.pr, body)

        section_names = []
        for line in body.split("\n"):
            if line.startswith("## "):
                section_names.append(line[3:])

        print(json.dumps({"status": "ok", "sections": section_names}))

    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))


if __name__ == "__main__":
    main()
