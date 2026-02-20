#!/usr/bin/env bash
# FLOW Process — SessionStart hook
#
# Scans .claude/flow-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → resets interrupted session timing, injects resume instruction
# 2+ files → asks user to pick a feature, then resumes

set -euo pipefail

STATE_DIR=".claude/flow-states"

# No state directory or no state files — exit silently
if [ ! -d "$STATE_DIR" ]; then
  exit 0
fi

if [ -z "$(ls "$STATE_DIR"/*.json 2>/dev/null)" ]; then
  exit 0
fi

# Reset any interrupted session timing and build context
CONTEXT=$(python3 - << 'PYTHON'
import json, sys
from pathlib import Path

state_dir = Path(".claude/flow-states")
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

    print(f"""<flow-session-resume>
FLOW feature in progress: "{feature}" — Phase {cp}: {phase_name}

Your FIRST action before responding to anything else:
Invoke the flow:resume skill.
</flow-session-resume>""")

else:
    features = []
    for s in states:
        cp = str(s.get("current_phase", "1"))
        phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
        features.append(f"{s.get('feature')} — Phase {cp}: {phase_name}")

    feature_list = "\n".join(f"  - {f}" for f in features)

    print(f"""<flow-session-resume>
Multiple FLOW features are in progress:
{feature_list}

Your FIRST action before responding to anything else:
Use AskUserQuestion to ask which feature to work on.
Once selected, cd into that feature's worktree then invoke the flow:resume skill.
</flow-session-resume>""")
PYTHON
)

if [ -z "$CONTEXT" ]; then
  exit 0
fi

escape_for_json() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

CONTEXT_ESCAPED=$(escape_for_json "$CONTEXT")

cat << EOF
{
  "additional_context": "${CONTEXT_ESCAPED}",
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "additionalContext": "${CONTEXT_ESCAPED}"
  }
}
EOF

exit 0
