"""Cleanup orchestrator for FLOW features.

Shared by /flow:flow-complete (Phase 6) and /flow:flow-abort. Performs best-effort
cleanup steps, continuing on failure.

Usage:
  bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>]

Output (JSON to stdout):
  {"status": "ok", "steps": {"worktree": "removed", "state_file": "deleted", ...}}

Each step reports one of: "removed"/"deleted"/"closed", "skipped", or "failed: <reason>".
"""

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


def _run_cmd(args, cwd):
    """Run a command, returning (success, output)."""
    try:
        result = subprocess.run(
            args,
            capture_output=True,
            text=True,
            cwd=str(cwd),
        )
        if result.returncode != 0:
            error = result.stderr.strip() or result.stdout.strip()
            return False, error
        return True, result.stdout.strip()
    except Exception as e:
        return False, str(e)


def cleanup(project_root, branch, worktree, pr_number=None, pull=False):
    """Perform cleanup steps. Returns a dict of step results."""
    root = Path(project_root)
    steps = {}

    # Close PR (abort only)
    if pr_number:
        ok, output = _run_cmd(
            ["gh", "pr", "close", str(pr_number)],
            root,
        )
        steps["pr_close"] = "closed" if ok else f"failed: {output}"
    else:
        steps["pr_close"] = "skipped"

    # Remove worktree tmp/ (FLOW repo only — before worktree removal)
    is_flow_repo = (root / "flow-phases.json").exists()
    wt_tmp = root / worktree / "tmp"
    if is_flow_repo and wt_tmp.is_dir():
        try:
            shutil.rmtree(wt_tmp)
            steps["worktree_tmp"] = "removed"
        except Exception as e:
            steps["worktree_tmp"] = f"failed: {e}"
    else:
        steps["worktree_tmp"] = "skipped"

    # Remove worktree
    wt_path = root / worktree
    if wt_path.exists():
        ok, output = _run_cmd(
            ["git", "worktree", "remove", str(wt_path), "--force"],
            root,
        )
        steps["worktree"] = "removed" if ok else f"failed: {output}"
    else:
        steps["worktree"] = "skipped"

    # Delete remote branch
    ok, output = _run_cmd(
        ["git", "push", "origin", "--delete", branch],
        root,
    )
    steps["remote_branch"] = "deleted" if ok else f"failed: {output}"

    # Delete local branch
    ok, output = _run_cmd(
        ["git", "branch", "-D", branch],
        root,
    )
    steps["local_branch"] = "deleted" if ok else f"failed: {output}"

    # Delete state file
    state_file = root / ".flow-states" / f"{branch}.json"
    if state_file.exists():
        try:
            state_file.unlink()
            steps["state_file"] = "deleted"
        except Exception as e:
            steps["state_file"] = f"failed: {e}"
    else:
        steps["state_file"] = "skipped"

    # Delete plan file
    plan_file = root / ".flow-states" / f"{branch}-plan.md"
    if plan_file.exists():
        try:
            plan_file.unlink()
            steps["plan_file"] = "deleted"
        except Exception as e:
            steps["plan_file"] = f"failed: {e}"
    else:
        steps["plan_file"] = "skipped"

    # Delete DAG file
    dag_file = root / ".flow-states" / f"{branch}-dag.md"
    if dag_file.exists():
        try:
            dag_file.unlink()
            steps["dag_file"] = "deleted"
        except Exception as e:
            steps["dag_file"] = f"failed: {e}"
    else:
        steps["dag_file"] = "skipped"

    # Delete log file
    log_file = root / ".flow-states" / f"{branch}.log"
    if log_file.exists():
        try:
            log_file.unlink()
            steps["log_file"] = "deleted"
        except Exception as e:
            steps["log_file"] = f"failed: {e}"
    else:
        steps["log_file"] = "skipped"

    # Delete frozen phases file
    frozen_file = root / ".flow-states" / f"{branch}-phases.json"
    if frozen_file.exists():
        try:
            frozen_file.unlink()
            steps["frozen_phases"] = "deleted"
        except Exception as e:
            steps["frozen_phases"] = f"failed: {e}"
    else:
        steps["frozen_phases"] = "skipped"

    # Delete CI sentinel
    ci_sentinel = root / ".flow-states" / f"{branch}-ci-passed"
    if ci_sentinel.exists():
        try:
            ci_sentinel.unlink()
            steps["ci_sentinel"] = "deleted"
        except Exception as e:
            steps["ci_sentinel"] = f"failed: {e}"
    else:
        steps["ci_sentinel"] = "skipped"

    # Delete timings file
    timings_file = root / ".flow-states" / f"{branch}-timings.md"
    if timings_file.exists():
        try:
            timings_file.unlink()
            steps["timings_file"] = "deleted"
        except Exception as e:
            steps["timings_file"] = f"failed: {e}"
    else:
        steps["timings_file"] = "skipped"

    # Delete closed issues file
    closed_issues_file = root / ".flow-states" / f"{branch}-closed-issues.json"
    if closed_issues_file.exists():
        try:
            closed_issues_file.unlink()
            steps["closed_issues_file"] = "deleted"
        except Exception as e:
            steps["closed_issues_file"] = f"failed: {e}"
    else:
        steps["closed_issues_file"] = "skipped"

    # Delete issues file
    issues_file = root / ".flow-states" / f"{branch}-issues.md"
    if issues_file.exists():
        try:
            issues_file.unlink()
            steps["issues_file"] = "deleted"
        except Exception as e:
            steps["issues_file"] = f"failed: {e}"
    else:
        steps["issues_file"] = "skipped"

    # Pull latest main (after worktree removal — ordering matters)
    if pull:
        ok, output = _run_cmd(
            ["git", "pull", "origin", "main"],
            root,
        )
        steps["git_pull"] = "pulled" if ok else f"failed: {output}"

    return steps


def main():
    parser = argparse.ArgumentParser(description="FLOW cleanup orchestrator")
    parser.add_argument("project_root", help="Path to project root")
    parser.add_argument("--branch", required=True, help="Branch name")
    parser.add_argument("--worktree", required=True, help="Worktree path (relative)")
    parser.add_argument("--pr", type=int, default=None, help="PR number to close")
    parser.add_argument("--pull", action="store_true", help="Run git pull origin main after cleanup")
    args = parser.parse_args()

    root = Path(args.project_root)
    if not root.is_dir():
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Project root not found: {args.project_root}",
                }
            )
        )
        sys.exit(1)

    steps = cleanup(root, args.branch, args.worktree, args.pr, pull=args.pull)
    print(json.dumps({"status": "ok", "steps": steps}))


if __name__ == "__main__":
    main()
