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
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    AUTO_SKILLS,
    branch_name,
    build_initial_phases,
    detect_tty,
    freeze_phases,
    now,
    read_flow_json,
    read_prompt_file,
)
from log import append_log


def create_state(
    project_root, branch, framework="rails", skills=None, prompt="", start_step=None, start_steps_total=None
):
    """Create the initial state file with null PR fields."""
    current_time = now()
    phases = build_initial_phases(current_time)

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
        "session_tty": detect_tty(),
        "session_id": None,
        "transcript_path": None,
        "notes": [],
        "prompt": prompt,
        "phases": phases,
        "phase_transitions": [],
    }
    if skills is not None:
        state["skills"] = skills
    if start_step is not None:
        state["start_step"] = start_step
    if start_steps_total is not None:
        state["start_steps_total"] = start_steps_total

    state_dir = project_root / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    state_path = state_dir / f"{branch}.json"
    state_path.write_text(json.dumps(state, indent=2))
    return state


def main():
    parser = argparse.ArgumentParser(description="FLOW init-state — early state file creation")
    parser.add_argument("feature_name", nargs="?", help="Feature name words")
    parser.add_argument(
        "--prompt-file", default=None, help="Path to file containing start prompt (file is deleted after reading)"
    )
    parser.add_argument("--auto", action="store_true", help="Override all skills to fully autonomous preset")
    parser.add_argument("--start-step", type=int, default=None, help="Initial start_step value for TUI progress")
    parser.add_argument("--start-steps-total", type=int, default=None, help="Total start steps for TUI progress")
    args = parser.parse_args()

    if not args.feature_name:
        print(
            json.dumps(
                {
                    "status": "error",
                    "step": "args",
                    "message": 'Feature name required. Usage: bin/flow init-state "<feature name>"',
                }
            )
        )
        sys.exit(1)

    feature_words = args.feature_name
    branch = branch_name(feature_words)
    project_root = Path.cwd()

    # Read .flow.json for framework and skills
    init_data = read_flow_json(project_root)
    if init_data is None:
        print(
            json.dumps(
                {
                    "status": "error",
                    "step": "flow_json",
                    "message": "Could not read .flow.json",
                }
            )
        )
        sys.exit(1)

    framework = init_data.get("framework", "rails")
    skills = init_data.get("skills")
    if args.auto:
        skills = AUTO_SKILLS

    # Read prompt
    if args.prompt_file:
        raw_prompt, read_error = read_prompt_file(args.prompt_file)
        if read_error:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "step": "prompt_file",
                        "message": read_error,
                    }
                )
            )
            sys.exit(1)
    else:
        raw_prompt = feature_words

    # Create state file and frozen phases
    create_state(
        project_root,
        branch,
        framework=framework,
        skills=skills,
        prompt=raw_prompt,
        start_step=args.start_step,
        start_steps_total=args.start_steps_total,
    )
    append_log(branch, f"[Phase 1] create .flow-states/{branch}.json (exit 0)")

    freeze_phases(project_root, branch)
    append_log(branch, f"[Phase 1] freeze .flow-states/{branch}-phases.json (exit 0)")

    print(
        json.dumps(
            {
                "status": "ok",
                "branch": branch,
                "state_file": f".flow-states/{branch}.json",
            }
        )
    )


if __name__ == "__main__":
    main()
