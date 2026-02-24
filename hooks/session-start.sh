#!/usr/bin/env bash
# FLOW Process — SessionStart hook
#
# Scans .flow-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → resets interrupted session timing, injects resume instruction
# 2+ files → asks user to pick a feature, then resumes

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
    cp = str(state.get("current_phase", "1"))
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

if len(states) == 1:
    s = states[0]
    cp = str(s.get("current_phase", "1"))
    phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
    feature = s.get("feature", "")

    context = (
        "<flow-session-resume>\n"
        f'FLOW feature in progress: "{feature}" — Phase {cp}: {phase_name}\n'
        "\n"
        "Your FIRST action before responding to anything else:\n"
        "Invoke the flow:resume skill.\n"
        "\n"
        "Throughout this session: whenever the user corrects you, disagrees\n"
        "with your response, or says something was wrong, invoke flow:note\n"
        "immediately before replying to capture the correction.\n"
        "</flow-session-resume>"
    )

else:
    features = []
    for s in states:
        cp = str(s.get("current_phase", "1"))
        phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
        features.append(f"{s.get('feature')} — Phase {cp}: {phase_name}")

    feature_list = "\n".join(f"  - {f}" for f in features)

    context = (
        "<flow-session-resume>\n"
        "Multiple FLOW features are in progress:\n"
        f"{feature_list}\n"
        "\n"
        "Your FIRST action before responding to anything else:\n"
        "Use AskUserQuestion to ask which feature to work on.\n"
        "Once selected, cd into that feature's worktree then invoke the flow:resume skill.\n"
        "</flow-session-resume>"
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
