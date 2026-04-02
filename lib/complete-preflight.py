"""Consolidated preflight for FLOW Complete phase.

Absorbs SOFT-GATE + Steps 1-3: state detection, PR status check,
and merge main into branch.

Usage: bin/flow complete-preflight [--branch <name>] [--auto] [--manual]

Output (JSON to stdout):
  Success:  {"status": "ok", "mode": "auto", "pr_state": "OPEN", "merge": "clean", "warnings": []}
  Merged:   {"status": "ok", "pr_state": "MERGED", ...}
  Conflict: {"status": "conflict", "conflict_files": ["..."], ...}
  Inferred: {"status": "ok", "inferred": true, ...}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    LOCAL_TIMEOUT,
    NETWORK_TIMEOUT,
    current_branch,
    derive_worktree,
    mutate_state,
    parse_conflict_files,
    project_root,
)

BIN_FLOW = str(Path(__file__).resolve().parent.parent / "bin" / "flow")
COMPLETE_STEPS_TOTAL = 7


def _run_cmd(args, timeout=LOCAL_TIMEOUT):
    """Run a command, returning CompletedProcess. Never raises."""
    try:
        return subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=args,
            returncode=1,
            stdout="",
            stderr=f"Timed out after {timeout}s",
        )


def _resolve_mode(auto=False, manual=False, state=None):
    """Resolve mode from flags and state file.

    Priority: --auto > --manual > state file skills.flow-complete > default "auto".
    """
    if auto:
        return "auto"
    if manual:
        return "manual"
    if state:
        skills = state.get("skills", {})
        skill_config = skills.get("flow-complete")
        if isinstance(skill_config, str):
            return skill_config
        if isinstance(skill_config, dict):
            return skill_config.get("continue", "auto")
    return "auto"


def _check_learn_phase(state):
    """Check if Learn phase is complete. Returns list of warning strings."""
    warnings = []
    phases = state.get("phases", {})
    learn = phases.get("flow-learn", {})
    learn_status = learn.get("status", "pending")
    if learn_status != "complete":
        warnings.append(f"Phase 5 not complete (status: {learn_status}).")
    return warnings


def _phase_transition_enter(branch):
    """Call phase-transition --action enter via subprocess."""
    result = _run_cmd(
        [
            BIN_FLOW,
            "phase-transition",
            "--phase",
            "flow-complete",
            "--action",
            "enter",
            "--branch",
            branch,
        ]
    )
    if result.returncode != 0:
        return None, result.stderr.strip()
    try:
        return json.loads(result.stdout.strip()), None
    except (json.JSONDecodeError, ValueError):
        return None, f"Invalid JSON from phase-transition: {result.stdout}"


def _check_pr_status(pr_number=None, branch=None):
    """Check PR status via gh pr view. Returns state string or None on error."""
    if pr_number:
        identifier = str(pr_number)
    elif branch:
        identifier = branch
    else:
        return None, "No PR number or branch to check"
    result = _run_cmd(
        ["gh", "pr", "view", identifier, "--json", "state", "--jq", ".state"],
        timeout=NETWORK_TIMEOUT,
    )
    if result.returncode != 0:
        return None, result.stderr.strip() or "Could not find PR"
    return result.stdout.strip(), None


def _merge_main(branch):
    """Fetch and merge origin/main into the current branch.

    Returns one of:
      ("clean", None) — already up to date
      ("merged", None) — merged successfully (new commits)
      ("conflict", [files]) — merge conflicts
      ("error", message) — unexpected error
    """
    # Fetch
    result = _run_cmd(["git", "fetch", "origin", "main"], timeout=NETWORK_TIMEOUT)
    if result.returncode != 0:
        return "error", result.stderr.strip()

    # Check if already up to date
    result = _run_cmd(["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"])
    if result.returncode == 0:
        return "clean", None

    # Merge
    result = _run_cmd(["git", "merge", "origin/main"], timeout=NETWORK_TIMEOUT)
    if result.returncode == 0:
        # Merged successfully — push
        push_result = _run_cmd(["git", "push"], timeout=NETWORK_TIMEOUT)
        if push_result.returncode != 0:
            return "error", f"Merge succeeded but push failed: {push_result.stderr.strip()}"
        return "merged", None

    # Check for conflicts
    status_result = _run_cmd(["git", "status", "--porcelain"])
    conflict_files = parse_conflict_files(status_result.stdout)
    if conflict_files:
        return "conflict", conflict_files

    return "error", result.stderr.strip()


def preflight(branch=None, auto=False, manual=False, root=None):
    """Run the Complete phase preflight checks.

    Args:
        branch: Override branch name. Auto-detected from git if None.
        auto: Force auto mode.
        manual: Force manual mode.
        root: Project root path. Auto-detected via project_root() if None.

    Returns a result dict with status, mode, pr_state, merge result, and warnings.
    """
    if root is None:
        root = project_root()
    else:
        root = Path(root)

    # Resolve branch
    if branch is None:
        branch = current_branch()
    if not branch:
        return {"status": "error", "message": "Could not determine current branch"}

    # Read state file
    state_path = root / ".flow-states" / f"{branch}.json"
    state = None
    inferred = False

    if state_path.exists():
        try:
            state = json.loads(state_path.read_text())
        except (json.JSONDecodeError, ValueError):
            return {"status": "error", "message": f"Could not parse state file: {state_path}"}
    else:
        inferred = True

    # Resolve mode
    mode = _resolve_mode(auto=auto, manual=manual, state=state)

    # Warnings
    warnings = []
    if state:
        warnings = _check_learn_phase(state)

    # Phase transition enter (only if state file exists)
    if state:
        pt_result, pt_error = _phase_transition_enter(branch)
        if pt_error:
            return {"status": "error", "message": f"Phase transition failed: {pt_error}"}

        # Set step counters
        def _set_counters(s):
            s["complete_steps_total"] = COMPLETE_STEPS_TOTAL
            s["complete_step"] = 1

        mutate_state(state_path, _set_counters)

    # Check PR status
    pr_number = state.get("pr_number") if state else None
    pr_state, pr_error = _check_pr_status(pr_number=pr_number, branch=branch)
    if pr_error:
        return {"status": "error", "message": pr_error}

    base = {
        "mode": mode,
        "pr_state": pr_state,
        "warnings": warnings,
        "branch": branch,
    }
    if inferred:
        base["inferred"] = True
    if state:
        base["pr_number"] = pr_number
        base["pr_url"] = state.get("pr_url", "")
        base["worktree"] = derive_worktree(branch)

    # PR already merged — return early
    if pr_state == "MERGED":
        return {"status": "ok", **base}

    # PR closed — error
    if pr_state == "CLOSED":
        return {"status": "error", "message": "PR is closed but not merged. Reopen or create a new PR first.", **base}

    # PR not found
    if pr_state not in ("OPEN",):
        return {"status": "error", "message": f"Unexpected PR state: {pr_state}", **base}

    # Merge main into branch
    merge_status, merge_data = _merge_main(branch)

    if merge_status == "conflict":
        return {"status": "conflict", "conflict_files": merge_data, **base}
    if merge_status == "error":
        return {"status": "error", "message": merge_data, **base}

    base["merge"] = merge_status
    return {"status": "ok", **base}


def main():
    parser = argparse.ArgumentParser(description="FLOW Complete phase preflight")
    parser.add_argument("--branch", default=None, help="Override branch for state file lookup")
    parser.add_argument("--auto", action="store_true", help="Force auto mode")
    parser.add_argument("--manual", action="store_true", help="Force manual mode")
    args = parser.parse_args()

    result = preflight(branch=args.branch, auto=args.auto, manual=args.manual)
    print(json.dumps(result))

    if result["status"] not in ("ok",):
        sys.exit(1)


if __name__ == "__main__":
    main()
