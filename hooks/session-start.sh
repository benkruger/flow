#!/usr/bin/env bash
# FLOW Process — SessionStart hook
#
# Scans .flow-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → resets interrupted session timing, injects awareness context
# 2+ files → injects awareness context listing all features

set -euo pipefail

STATE_DIR=".flow-states"

# No state directory or no state files — exit silently
if [ ! -d "$STATE_DIR" ]; then
  exit 0
fi

if [ -z "$(ls "$STATE_DIR"/*.json 2>/dev/null)" ]; then
  exit 0
fi

# Reset any interrupted session timing, build context, and emit JSON output
python3 - << 'PYTHON'
import json, sys
from pathlib import Path

state_dir = Path(".flow-states")
files = sorted(state_dir.glob("*.json"))

if not files:
    sys.exit(0)


def reset_interrupted(path, state):
    cp = state.get("current_phase", "start")
    phase = state.get("phases", {}).get(cp, {})
    if phase.get("session_started_at") is not None:
        state["phases"][cp]["session_started_at"] = None
        with open(path, "w") as f:
            json.dump(state, f, indent=2)


states = []
for path in files:
    try:
        with open(path) as f:
            state = json.load(f)
        reset_interrupted(path, state)
        states.append(state)
    except Exception:
        continue

if not states:
    sys.exit(0)

dev_mode = (state_dir / ".dev-mode").exists()
dev_preamble = ""
if dev_mode:
    dev_preamble = (
        "[DEV MODE] FLOW plugin is running from local source.\n"
        "When printing any FLOW banner, add [DEV MODE] after the version number.\n"
        "\n"
    )

if len(states) == 1:
    s = states[0]
    cp = s.get("current_phase", "start")
    phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
    feature = s.get("feature", "")
    plan_file = s.get("plan_file")
    plan_approved = cp == "plan" and plan_file is not None

    if plan_approved:
        resume_instruction = (
            "The plan was approved and ExitPlanMode cleared context.\n"
            "Invoke flow:continue immediately to complete Phase 2 and "
            "transition to Phase 3: Code.\n"
        )
    else:
        resume_instruction = (
            "Do NOT invoke flow:continue or ask about this feature unprompted.\n"
            "The user will type /flow:continue when ready to resume.\n"
        )

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        f'FLOW feature in progress: "{feature}" — {phase_name}\n'
        "\n"
        f"{resume_instruction}"
        "\n"
        "Throughout this session: whenever the user corrects you, disagrees\n"
        "with your response, or says something was wrong, invoke flow:note\n"
        "immediately before replying to capture the correction.\n"
        "</flow-session-context>"
    )

else:
    features = []
    for s in states:
        cp = s.get("current_phase", "start")
        phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
        features.append(f"{s.get('feature')} — {phase_name}")

    feature_list = "\n".join(f"  - {f}" for f in features)

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        "Multiple FLOW features are in progress:\n"
        f"{feature_list}\n"
        "\n"
        "Do NOT invoke flow:continue or ask about these features unprompted.\n"
        "The user will type /flow:continue when ready to resume.\n"
        "\n"
        "Throughout this session: whenever the user corrects you, disagrees\n"
        "with your response, or says something was wrong, invoke flow:note\n"
        "immediately before replying to capture the correction.\n"
        "</flow-session-context>"
    )

output = {
    "additional_context": context,
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": context,
    },
}
print(json.dumps(output))
PYTHON

exit 0
