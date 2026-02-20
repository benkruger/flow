#!/usr/bin/env bash
# SDLC Process — SessionStart hook
#
# Scans .claude/sdlc-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → injects context to cd into worktree and resume
# 2+ files → injects context to ask user which feature to resume

set -euo pipefail

STATE_DIR=".claude/sdlc-states"

# No state directory or no state files — exit silently
if [ ! -d "$STATE_DIR" ]; then
  exit 0
fi

if [ -z "$(ls "$STATE_DIR"/*.json 2>/dev/null)" ]; then
  exit 0
fi

# Use Python to read state files, reset interrupted sessions, build context
CONTEXT=$(python3 - << 'PYTHON'
import json, sys
from pathlib import Path

state_dir = Path(".claude/sdlc-states")
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
    worktree = s.get("worktree", "")
    feature = s.get("feature", "")
    pr_url = s.get("pr_url", "")

    print(f"""<sdlc-session-resume>
SDLC feature in progress: "{feature}"
Current phase: {cp} — {phase_name}
PR: {pr_url}

Your FIRST actions before responding to anything else:
1. Run: cd {worktree}
2. Rebuild task list using TaskCreate for each phase — completed phases
   as completed, phase {cp} ({phase_name}) as in_progress, rest as pending
3. Print the SDLC status banner showing where we are

Do this before anything else.
</sdlc-session-resume>""")

else:
    lines = [
        "<sdlc-session-resume>",
        "Multiple SDLC features are in progress.",
        "",
        "Your FIRST action before responding to anything else:",
        "Use AskUserQuestion to ask: \"Which feature would you like to work on?\"",
        "Present each feature as an option:",
        "",
    ]
    for s in states:
        cp = str(s.get("current_phase", "1"))
        phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
        lines.append(f"  - {s.get('feature')} — Phase {cp}: {phase_name}")

    lines += [
        "",
        "Once selected:",
        "1. cd into that feature's worktree",
        "2. Rebuild task list",
        "3. Print SDLC status banner",
        "</sdlc-session-resume>"
    ]
    print("\n".join(lines))
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
