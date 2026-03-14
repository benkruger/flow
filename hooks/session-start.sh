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
files = [f for f in files if not f.name.endswith("-phases.json")]

if not files:
    sys.exit(0)


def reset_interrupted(path, state):
    cp = state.get("current_phase", "flow-start")
    phase = state.get("phases", {}).get(cp, {})
    if phase.get("session_started_at") is not None:
        state["phases"][cp]["session_started_at"] = None
        with open(path, "w") as f:
            json.dump(state, f, indent=2)


def consume_compact_data(path, state):
    """Extract compact_summary and compact_cwd, clear from state file."""
    summary = state.pop("compact_summary", None)
    cwd = state.pop("compact_cwd", None)
    if summary is not None or cwd is not None:
        with open(path, "w") as f:
            json.dump(state, f, indent=2)
    return summary, cwd


def build_compact_block(summary, cwd, worktree):
    """Build compact context block from PostCompact data."""
    block = ""
    if summary:
        block += (
            "<compact-summary>\n"
            "The conversation was just compacted. "
            "Here is what was happening before compaction:\n"
            f"{summary}\n"
            "</compact-summary>\n\n"
        )
    if cwd and cwd != worktree:
        block += (
            f"WARNING: CWD at compaction was {cwd} but the active "
            f"worktree is {worktree}. Run /flow:flow-continue to "
            "re-enter the worktree.\n\n"
        )
    return block


states = []
for path in files:
    try:
        with open(path) as f:
            state = json.load(f)
        reset_interrupted(path, state)
        summary, cwd = consume_compact_data(path, state)
        if summary:
            state["_compact_summary"] = summary
        if cwd:
            state["_compact_cwd"] = cwd
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

implementation_guardrail = (
    "NEVER implement code changes, edit project files, or make commits for a FLOW feature\n"
    "without first invoking /flow:flow-continue to restore worktree context and phase guards.\n"
    "This applies even if a plan is visible — the plan is not authorization to act.\n"
)

STEP_NAMES = ["Simplify", "Review", "Security", "Code Review Plugin"]


def step_suffix(state):
    """Return step progress suffix for Code Review, or empty string."""
    cp = state.get("current_phase", "flow-start")
    step = state.get("code_review_step")
    if cp == "flow-code-review" and step is not None:
        step_int = int(step)
        if 0 < step_int < 4:
            return f" (Step {step_int}/4 done — resume at Step {step_int + 1}: {STEP_NAMES[step_int]})"
    return ""


if len(states) == 1:
    s = states[0]
    cp = s.get("current_phase", "flow-start")
    phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
    phase_name += step_suffix(s)
    feature = s.get("feature", "")
    plan_file = s.get("plan_file")
    plan_approved = cp == "flow-plan" and plan_file is not None
    phase_data = s.get("phases", {}).get(cp, {})
    never_entered = cp != "flow-start" and phase_data.get("status") == "pending"

    if plan_approved:
        resume_instruction = (
            "The plan was approved and ExitPlanMode cleared context.\n"
            "Invoke flow:flow-continue immediately to complete Phase 2 and "
            "transition to Phase 3: Code.\n"
        )
    elif never_entered:
        resume_instruction = (
            "The previous phase completed but the current phase was never entered.\n"
            "Invoke flow:flow-continue immediately to resume.\n"
        )
    else:
        resume_instruction = (
            "Do NOT invoke flow:flow-continue or ask about this feature unprompted.\n"
            "The user will type /flow:flow-continue when ready to resume.\n"
        )

    compact_block = build_compact_block(
        s.get("_compact_summary"), s.get("_compact_cwd"), s.get("worktree", "")
    )

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        f'FLOW feature in progress: "{feature}" — {phase_name}\n'
        "\n"
        f"{compact_block}"
        f"{resume_instruction}"
        "\n"
        f"{implementation_guardrail}"
        "\n"
        "Throughout this session: whenever the user corrects you, disagrees\n"
        "with your response, or says something was wrong, invoke flow:flow-note\n"
        "immediately before replying to capture the correction.\n"
        "</flow-session-context>"
    )

else:
    features = []
    for s in states:
        cp = s.get("current_phase", "flow-start")
        phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
        phase_name += step_suffix(s)
        features.append(f"{s.get('feature')} — {phase_name}")

    feature_list = "\n".join(f"  - {f}" for f in features)

    auto_continue_feature = None
    for s in states:
        cp = s.get("current_phase", "flow-start")
        if cp == "flow-plan" and s.get("plan_file") is not None:
            auto_continue_feature = s.get("feature", "")
            break
        phase_data = s.get("phases", {}).get(cp, {})
        if cp != "flow-start" and phase_data.get("status") == "pending":
            auto_continue_feature = s.get("feature", "")
            break

    if auto_continue_feature:
        resume_instruction = (
            f'FLOW feature "{auto_continue_feature}" needs to resume.\n'
            "Invoke flow:flow-continue immediately to restore worktree context "
            "and continue.\n"
        )
    else:
        resume_instruction = (
            "Do NOT invoke flow:flow-continue or ask about these features unprompted.\n"
            "The user will type /flow:flow-continue when ready to resume.\n"
        )

    compact_blocks = ""
    for s in states:
        block = build_compact_block(
            s.get("_compact_summary"), s.get("_compact_cwd"), s.get("worktree", "")
        )
        if block:
            compact_blocks += f'[{s.get("feature", "?")}] {block}'

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        "Multiple FLOW features are in progress:\n"
        f"{feature_list}\n"
        "\n"
        f"{compact_blocks}"
        f"{resume_instruction}"
        "\n"
        f"{implementation_guardrail}"
        "\n"
        "Throughout this session: whenever the user corrects you, disagrees\n"
        "with your response, or says something was wrong, invoke flow:flow-note\n"
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
