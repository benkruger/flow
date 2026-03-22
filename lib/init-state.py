"""Create the initial FLOW state file with null PR fields.

Called early in the Start phase (before locked main operations) so the
TUI can see the flow immediately. PR fields are backfilled later by
start-setup.py after worktree + PR creation.

Usage: bin/flow init-state "<feature name>" [--prompt-file <path>] [--auto]

Output (JSON to stdout):
  Success: {"status": "ok", "branch": "...", "state_file": "..."}
  Failure: {"status": "error", "step": "...", "message": "..."}
"""

import argparse
import json
import os
import re
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, PHASE_NAMES, PHASE_ORDER
from log import append_log

PLUGIN_ROOT = Path(__file__).resolve().parent.parent

AUTO_SKILLS = {
    "flow-start": {"continue": "auto"},
    "flow-plan": {"continue": "auto", "dag": "auto"},
    "flow-code": {"commit": "auto", "continue": "auto"},
    "flow-code-review": {"commit": "auto", "continue": "auto", "code_review_plugin": "never"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto",
}


def _branch_name(feature_words):
    """Convert feature words to a hyphenated branch name, max 32 chars."""
    sanitized = re.sub(r"[^a-zA-Z0-9\s-]", "", feature_words)
    name = "-".join(sanitized.lower().split())
    if len(name) <= 32:
        return name
    truncated = name[:33]
    last_hyphen = truncated.rfind("-")
    if last_hyphen > 0:
        return truncated[:last_hyphen]
    return name[:32]


def _read_prompt_file(path):
    """Read prompt text from a file and delete the file.

    Returns (prompt_text, error_message). On success error is None.
    """
    try:
        with open(path) as fh:
            content = fh.read()
    except (OSError, IOError) as exc:
        return None, f"Could not read prompt file '{path}': {exc}"

    try:
        os.remove(path)
    except OSError:
        pass

    return content, None


def create_state(project_root, branch, framework="rails", skills=None,
                 prompt=""):
    """Create the initial state file with null PR fields."""
    current_time = now()
    phases = {}
    first_phase = PHASE_ORDER[0]
    for key in PHASE_ORDER:
        if key == first_phase:
            phases[key] = {
                "name": PHASE_NAMES[key],
                "status": "in_progress",
                "started_at": current_time,
                "completed_at": None,
                "session_started_at": current_time,
                "cumulative_seconds": 0,
                "visit_count": 1,
            }
        else:
            phases[key] = {
                "name": PHASE_NAMES[key],
                "status": "pending",
                "started_at": None,
                "completed_at": None,
                "session_started_at": None,
                "cumulative_seconds": 0,
                "visit_count": 0,
            }

    state = {
        "schema_version": 1,
        "branch": branch,
        "repo": None,
        "pr_number": None,
        "pr_url": None,
        "started_at": current_time,
        "current_phase": "flow-start",
        "framework": framework,
        "files": {
            "plan": None,
            "dag": None,
            "log": f".flow-states/{branch}.log",
            "state": f".flow-states/{branch}.json",
        },
        "session_id": None,
        "transcript_path": None,
        "notes": [],
        "prompt": prompt,
        "phases": phases,
        "phase_transitions": [],
    }
    if skills is not None:
        state["skills"] = skills

    state_dir = project_root / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    state_path = state_dir / f"{branch}.json"
    state_path.write_text(json.dumps(state, indent=2))
    return state


def freeze_phases(project_root, branch):
    """Copy flow-phases.json to .flow-states/<branch>-phases.json."""
    source = PLUGIN_ROOT / "flow-phases.json"
    dest_dir = project_root / ".flow-states"
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = dest_dir / f"{branch}-phases.json"
    shutil.copy2(source, dest)


def main():
    parser = argparse.ArgumentParser(description="FLOW init-state — early state file creation")
    parser.add_argument("feature_name", nargs="?", help="Feature name words")
    parser.add_argument("--prompt-file", default=None,
                        help="Path to file containing start prompt (file is deleted after reading)")
    parser.add_argument("--auto", action="store_true",
                        help="Override all skills to fully autonomous preset")
    args = parser.parse_args()

    if not args.feature_name:
        print(json.dumps({
            "status": "error",
            "step": "args",
            "message": "Feature name required. Usage: bin/flow init-state \"<feature name>\"",
        }))
        sys.exit(1)

    feature_words = args.feature_name
    branch = _branch_name(feature_words)
    project_root = Path.cwd()

    # Read .flow.json for framework and skills
    flow_json_path = project_root / ".flow.json"
    try:
        init_data = json.loads(flow_json_path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(json.dumps({
            "status": "error",
            "step": "flow_json",
            "message": f"Could not read .flow.json: {exc}",
        }))
        sys.exit(1)

    framework = init_data.get("framework", "rails")
    skills = init_data.get("skills")
    if args.auto:
        skills = AUTO_SKILLS

    # Read prompt
    if args.prompt_file:
        raw_prompt, read_error = _read_prompt_file(args.prompt_file)
        if read_error:
            print(json.dumps({
                "status": "error",
                "step": "prompt_file",
                "message": read_error,
            }))
            sys.exit(1)
    else:
        raw_prompt = feature_words

    # Create state file and frozen phases
    create_state(project_root, branch, framework=framework, skills=skills,
                 prompt=raw_prompt)
    append_log(branch, f"[Phase 1] create .flow-states/{branch}.json (exit 0)")

    freeze_phases(project_root, branch)
    append_log(branch, f"[Phase 1] freeze .flow-states/{branch}-phases.json (exit 0)")

    print(json.dumps({
        "status": "ok",
        "branch": branch,
        "state_file": f".flow-states/{branch}.json",
    }))


if __name__ == "__main__":
    main()
