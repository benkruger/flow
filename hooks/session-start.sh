#!/usr/bin/env bash
# ROR Process — SessionStart hook
# Detects an in-progress ROR feature, resets interrupted session timing,
# and injects context so Claude rebuilds the task list automatically.

set -euo pipefail

STATE_FILE=".claude/ror-state.json"

# No ROR feature in progress — exit silently
if [ ! -f "$STATE_FILE" ]; then
  exit 0
fi

# Reset session_started_at if set from an interrupted session
python3 - << 'PYTHON'
import json, sys

try:
    with open(".claude/ror-state.json") as f:
        state = json.load(f)

    cp = str(state.get("current_phase", "1"))
    phase = state.get("phases", {}).get(cp, {})

    if phase.get("session_started_at") is not None:
        state["phases"][cp]["session_started_at"] = None
        with open(".claude/ror-state.json", "w") as f:
            json.dump(state, f, indent=2)
except Exception:
    pass
PYTHON

escape_for_json() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

CONTEXT="<ror-session-resume>
A ROR feature is in progress in this project.

On your FIRST reply, before doing anything else:
1. Read .claude/ror-state.json
2. Create a task for each phase using TaskCreate — mark completed phases as completed, the current phase as in_progress, remaining phases as pending
3. Print the ROR status banner so the user knows immediately where they are

Do this before responding to anything else.
</ror-session-resume>"

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
