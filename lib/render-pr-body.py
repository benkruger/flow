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
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import current_branch, extract_issue_numbers, project_root


def _load_sibling(name, filename):
    """Import a hyphenated sibling module via importlib."""
    lib_dir = Path(__file__).resolve().parent
    spec = importlib.util.spec_from_file_location(name, lib_dir / filename)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


_timings_mod = _load_sibling("format_pr_timings", "format-pr-timings.py")
_issues_mod = _load_sibling("format_issues_summary", "format-issues-summary.py")
_upb_mod = _load_sibling("update_pr_body", "update-pr-body.py")

_build_details = _upb_mod._build_details_block
_build_plain_section = _upb_mod._build_plain_section
_gh_set_body = _upb_mod._gh_set_body


def _resolve_path(path_str, project_dir):
    """Resolve a file path, handling both absolute and relative paths.

    Returns a Path object or None if path_str is falsy.
    Relative paths are resolved against project_dir.
    """
    if not path_str:
        return None
    p = Path(path_str)
    if p.is_absolute():
        return p
    return Path(project_dir) / p


def _build_artifacts(state):
    """Build the ## Artifacts section from state fields.

    Prefers the structured files block (relative paths) when present.
    Falls back to legacy top-level plan_file/dag_file for old state files.
    """
    files = state.get("files")
    if files:
        rows = [
            "| File | Path |",
            "|------|------|",
        ]
        labels = [("Plan", "plan"), ("DAG", "dag"), ("Log", "log"), ("State", "state")]
        for label, key in labels:
            path = files.get(key)
            if path:
                rows.append(f"| {label} | `{path}` |")
        transcript = state.get("transcript_path")
        if transcript:
            rows.append(f"| Transcript | `{transcript}` |")
        return rows if len(rows) > 2 else []

    # Legacy: top-level plan_file/dag_file
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

    # 1. What (always) — requires prompt field from start-setup
    what_text = state.get("prompt")
    if not what_text:
        raise ValueError("State file missing 'prompt' field — start-setup.py should always set this")
    what_section = f"## What\n\n{what_text}."
    issue_numbers = extract_issue_numbers(what_text)
    if issue_numbers:
        closing_lines = "\n".join(f"Closes #{n}" for n in issue_numbers)
        what_section += f"\n\n{closing_lines}"
    sections.append(what_section)
    section_names.append("What")

    # 2. Artifacts (always, items conditional)
    artifact_items = _build_artifacts(state)
    if artifact_items:
        sections.append("## Artifacts\n\n" + "\n".join(artifact_items))
    else:
        sections.append("## Artifacts")
    section_names.append("Artifacts")

    # Resolve artifact paths from files block with legacy fallback
    files = state.get("files", {})
    plan_path = _resolve_path(files.get("plan") or state.get("plan_file"), project_dir)
    dag_path = _resolve_path(files.get("dag") or state.get("dag_file"), project_dir)

    # 3. Plan (conditional)
    if plan_path and plan_path.exists():
        content = plan_path.read_text().rstrip("\n")
        sections.append(_build_details("Plan", "Implementation plan", content, "text"))
        section_names.append("Plan")

    # 4. DAG Analysis (conditional, always text format)
    if dag_path and dag_path.exists():
        content = dag_path.read_text().rstrip("\n")
        sections.append(_build_details("DAG Analysis", "Decompose plugin output", content, "text"))
        section_names.append("DAG Analysis")

    # 5. Phase Timings (always, started phases only)
    timings_table = _timings_mod.format_timings_table(state, started_only=True)
    sections.append(_build_plain_section("Phase Timings", timings_table))
    section_names.append("Phase Timings")

    # 6. State File (always)
    state_json = json.dumps(state, indent=2)
    branch = state.get("branch", "unknown")
    sections.append(_build_details("State File", f".flow-states/{branch}.json", state_json, "json"))
    section_names.append("State File")

    # 7. Session Log (conditional)
    log_path = _resolve_path(
        files.get("log") or f".flow-states/{state.get('branch', 'unknown')}.log",
        project_dir,
    )
    if log_path and log_path.exists():
        log_rel = files.get("log") or f".flow-states/{state.get('branch', 'unknown')}.log"
        content = log_path.read_text().rstrip("\n")
        sections.append(_build_details("Session Log", log_rel, content, "text"))
        section_names.append("Session Log")

    # 8. Issues Filed (conditional)
    issues_table = _format_issues_table(state)
    if issues_table:
        sections.append(_build_plain_section("Issues Filed", issues_table))
        section_names.append("Issues Filed")

    return "\n\n".join(sections)


def main():
    parser = argparse.ArgumentParser(description="Render complete PR body from state")
    parser.add_argument("--pr", type=int, required=True, help="PR number")
    parser.add_argument("--state-file", help="Path to state file (auto-detected if omitted)")
    parser.add_argument("--dry-run", action="store_true", help="Generate body and return sections without updating PR")

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
