#!/usr/bin/env bash
# FLOW Process — SessionStart hook
#
# Scans .flow-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → resets interrupted session timing, injects awareness context
# 2+ files → injects awareness context listing all features

set -euo pipefail

STATE_DIR=".flow-states"
FLOW_PLUGIN_LIB="$(cd "$(dirname "$0")/../lib" 2>/dev/null && pwd)" || true
export FLOW_PLUGIN_LIB

# No state directory or no state files — exit silently unless FLOW-enabled
if [ ! -d "$STATE_DIR" ] && [ ! -f ".flow.json" ]; then
  exit 0
fi

if [ -d "$STATE_DIR" ] && [ -z "$(ls "$STATE_DIR"/*.json 2>/dev/null)" ] && [ ! -f ".flow.json" ]; then
  exit 0
fi

# Reset any interrupted session timing, build context, and emit JSON output
python3 - << 'PYTHON'
import json, os, re, subprocess, sys
from pathlib import Path

# Import flow_utils from the plugin lib directory
_flow_lib = os.environ.get("FLOW_PLUGIN_LIB", "")
_has_flow_utils = False
if _flow_lib:
    sys.path.insert(0, _flow_lib)
    try:
        from flow_utils import write_tab_sequences, detect_repo
        _has_flow_utils = True
    except ImportError:
        pass


state_dir = Path(".flow-states")
if state_dir.is_dir():
    files = sorted(state_dir.glob("*.json"))
    files = [f for f in files if not f.name.endswith("-phases.json")]
else:
    files = []


def detect_orchestrate():
    """Detect orchestrate.json, return context block. Cleans up completed runs."""
    orch_path = state_dir / "orchestrate.json"
    if not orch_path.exists():
        return ""

    try:
        with open(orch_path) as f:
            orch = json.load(f)
    except Exception:
        return ""

    if orch.get("completed_at") is not None:
        # Completed: inject morning report, then clean up
        summary = ""
        summary_path = state_dir / "orchestrate-summary.md"
        if summary_path.exists():
            summary = summary_path.read_text()

        block = (
            "<flow-orchestrate-report>\n"
            "FLOW orchestration completed. Present this report to the user:\n\n"
            f"{summary}\n"
            "</flow-orchestrate-report>\n"
        )

        # Clean up orchestrator files
        for name in ["orchestrate.json", "orchestrate-summary.md", "orchestrate.log", "orchestrate-queue.json"]:
            p = state_dir / name
            if p.exists():
                p.unlink()

        return block

    # All items processed — orchestrator finishing, no resume needed
    queue = orch.get("queue", [])
    if queue and all(item.get("outcome") is not None for item in queue):
        return ""

    # In-progress: inject resume context with queue position
    current_index = orch.get("current_index")
    current_issue = "(unknown)"
    if current_index is not None and 0 <= current_index < len(queue):
        item = queue[current_index]
        current_issue = f"#{item.get('issue_number', '?')} ({item.get('title', '')})"

    completed = sum(1 for item in queue if item.get("outcome") == "completed")
    total = len(queue)

    return (
        "<flow-orchestrate-context>\n"
        f"FLOW orchestration in progress. Processing issue {current_issue}.\n"
        f"Progress: {completed}/{total} completed.\n"
        "Resume the orchestrator by invoking flow:flow-orchestrate --continue-step.\n"
        "</flow-orchestrate-context>\n"
    )


orchestrate_block = detect_orchestrate()

# Exclude orchestrate.json from normal feature state processing
files = [f for f in files if f.name != "orchestrate.json"]

# Branch isolation: only process state files matching the current branch.
# Fail-open: if branch detection fails (detached HEAD, non-git), scan all.
try:
    _br = subprocess.run(
        ["git", "branch", "--show-current"],
        capture_output=True, text=True, check=True,
    )
    _current = _br.stdout.strip() or None
except Exception:
    _current = None

if _current:
    filtered = [f for f in files if f.stem == _current]
    if filtered:
        files = filtered
    # else: on main (or branch without state file) — keep all files

if not files and not orchestrate_block:
    if _has_flow_utils:
        try:
            write_tab_sequences(repo=detect_repo())
        except Exception:
            pass
    sys.exit(0)


def reset_interrupted(path, state):
    changed = False
    if "_blocked" in state:
        state.pop("_blocked")
        changed = True
    cp = state.get("current_phase", "flow-start")
    phase = state.get("phases", {}).get(cp, {})
    session_started = phase.get("session_started_at")
    if session_started is not None:
        try:
            from datetime import datetime
            from zoneinfo import ZoneInfo
            started_dt = datetime.fromisoformat(session_started)
            now_dt = datetime.now(ZoneInfo("America/Los_Angeles"))
            elapsed = max(0, int((now_dt - started_dt).total_seconds()))
            existing = phase.get("cumulative_seconds", 0)
            state["phases"][cp]["cumulative_seconds"] = existing + elapsed
            state["phases"][cp]["session_started_at"] = now_dt.isoformat(timespec="seconds")
        except Exception:
            state["phases"][cp]["session_started_at"] = None
        changed = True
    if changed:
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


def consume_last_failure(path, state):
    """Extract _last_failure, clear from state file."""
    failure = state.pop("_last_failure", None)
    if failure is not None:
        with open(path, "w") as f:
            json.dump(state, f, indent=2)
    return failure


def build_failure_block(failure):
    """Build failure context block from StopFailure data."""
    if not failure:
        return ""
    f_type = failure.get("type", "unknown")
    f_ts = failure.get("timestamp", "unknown")
    f_msg = failure.get("message", "")
    return (
        f"Previous session ended due to {f_type} at {f_ts}"
        + (f": {f_msg}\n\n" if f_msg else "\n\n")
    )


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
        if _current and path.stem == _current:
            try:
                _pid = os.getpid()
                _tty_name = None
                for _ in range(20):
                    _tr = subprocess.run(
                        ["ps", "-o", "tty=,ppid=", "-p", str(_pid)],
                        capture_output=True, text=True, timeout=5,
                    )
                    if _tr.returncode != 0:
                        break
                    _parts = _tr.stdout.strip().split()
                    if len(_parts) < 2:
                        break
                    if _parts[0] not in ("??", "?"):
                        _tty_name = "/dev/" + _parts[0]
                        break
                    _pid = int(_parts[1])
                    if _pid <= 1:
                        break
                if _tty_name and state.get("session_tty") != _tty_name:
                    state["session_tty"] = _tty_name
                    with open(path, "w") as wf:
                        json.dump(state, wf, indent=2)
            except Exception:
                pass
        failure = consume_last_failure(path, state)
        summary, cwd = consume_compact_data(path, state)
        if failure:
            state["_last_failure_consumed"] = failure
        if summary:
            state["_compact_summary"] = summary
        if cwd:
            state["_compact_cwd"] = cwd
        states.append(state)
    except Exception:
        continue

if not states and not orchestrate_block:
    if _has_flow_utils:
        try:
            write_tab_sequences(repo=detect_repo())
        except Exception:
            pass
    sys.exit(0)

dev_mode = False
try:
    flow_json_path = Path(".flow.json")
    if flow_json_path.exists():
        dev_mode = "plugin_root_backup" in json.loads(flow_json_path.read_text())
except (json.JSONDecodeError, OSError):
    pass
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


def _feature(state):
    branch = state.get("branch", "")
    return " ".join(w.capitalize() for w in branch.replace("-", " ").split())


def _worktree(state):
    return f".worktrees/{state.get('branch', '')}"


def step_suffix(state):
    """Return step progress suffix for Code Review, or empty string."""
    cp = state.get("current_phase", "flow-start")
    step = state.get("code_review_step")
    if cp == "flow-code-review" and step is not None:
        try:
            step_int = int(step)
        except (ValueError, TypeError):
            return ""
        if 0 < step_int < 4:
            return f" (Step {step_int}/4 done — resume at Step {step_int + 1}: {STEP_NAMES[step_int]})"
    return ""


if len(states) == 1:
    s = states[0]
    cp = s.get("current_phase", "flow-start")
    phase_name = s.get("phases", {}).get(cp, {}).get("name", "")
    phase_name += step_suffix(s)
    feature = _feature(s)
    plan_file = s.get("plan_file") or (s.get("files") or {}).get("plan")
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
        s.get("_compact_summary"), s.get("_compact_cwd"), _worktree(s)
    )
    failure_block = build_failure_block(s.get("_last_failure_consumed"))

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        f'FLOW feature in progress: "{feature}" — {phase_name}\n'
        "\n"
        f"{failure_block}"
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
        features.append(f"{_feature(s)} — {phase_name}")

    feature_list = "\n".join(f"  - {f}" for f in features)

    auto_continue_feature = None
    for s in states:
        cp = s.get("current_phase", "flow-start")
        if cp == "flow-plan" and (s.get("plan_file") or (s.get("files") or {}).get("plan")) is not None:
            auto_continue_feature = _feature(s)
            break
        phase_data = s.get("phases", {}).get(cp, {})
        if cp != "flow-start" and phase_data.get("status") == "pending":
            auto_continue_feature = _feature(s)
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

    failure_blocks = ""
    compact_blocks = ""
    for s in states:
        f_block = build_failure_block(s.get("_last_failure_consumed"))
        if f_block:
            failure_blocks += f'[{_feature(s)}] {f_block}'
        block = build_compact_block(
            s.get("_compact_summary"), s.get("_compact_cwd"), _worktree(s)
        )
        if block:
            compact_blocks += f'[{_feature(s)}] {block}'

    context = (
        "<flow-session-context>\n"
        f"{dev_preamble}"
        "Multiple FLOW features are in progress:\n"
        f"{feature_list}\n"
        "\n"
        f"{failure_blocks}"
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

try:
    if _has_flow_utils and states:
        write_tab_sequences(states[0])
    elif _has_flow_utils:
        write_tab_sequences(repo=detect_repo())
except Exception:
    pass

if orchestrate_block and not states:
    # Only orchestrate context, no feature states
    context = orchestrate_block
elif orchestrate_block:
    # Both orchestrate and feature context
    context = orchestrate_block + "\n" + context

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
