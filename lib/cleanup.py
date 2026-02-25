"""Cleanup orchestrator for FLOW features.

Shared by /flow:cleanup (Phase 8) and /flow:abort. Performs best-effort
cleanup steps, continuing on failure.

Usage:
  bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>] [--delete-remote]

Output (JSON to stdout):
  {"status": "ok", "steps": {"worktree": "removed", "state_file": "deleted", ...}}

Each step reports one of: "removed"/"deleted"/"closed", "skipped", or "failed: <reason>".
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path


def _run_cmd(args, cwd):
    """Run a command, returning (success, output)."""
    try:
        result = subprocess.run(
            args, capture_output=True, text=True, cwd=str(cwd),
        )
        if result.returncode != 0:
            error = result.stderr.strip() or result.stdout.strip()
            return False, error
        return True, result.stdout.strip()
    except Exception as e:
        return False, str(e)


def cleanup(project_root, branch, worktree, pr_number=None, delete_remote=False):
    """Perform cleanup steps. Returns a dict of step results."""
    root = Path(project_root)
    steps = {}

    # Close PR (abort only)
    if pr_number:
        ok, output = _run_cmd(
            ["gh", "pr", "close", str(pr_number),
             "--comment", "Aborted via /flow:abort"],
            root,
        )
        steps["pr_close"] = "closed" if ok else f"failed: {output}"
    else:
        steps["pr_close"] = "skipped"

    # Remove worktree
    wt_path = root / worktree
    if wt_path.exists():
        ok, output = _run_cmd(
            ["git", "worktree", "remove", str(worktree), "--force"],
            root,
        )
        steps["worktree"] = "removed" if ok else f"failed: {output}"
    else:
        steps["worktree"] = "skipped"

    # Delete remote branch (abort only)
    if delete_remote:
        ok, output = _run_cmd(
            ["git", "push", "origin", "--delete", branch],
            root,
        )
        steps["remote_branch"] = "deleted" if ok else f"failed: {output}"
    else:
        steps["remote_branch"] = "skipped"

    # Delete local branch (abort only)
    if delete_remote:
        ok, output = _run_cmd(
            ["git", "branch", "-D", branch],
            root,
        )
        steps["local_branch"] = "deleted" if ok else f"failed: {output}"
    else:
        steps["local_branch"] = "skipped"

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

    return steps


def main():
    parser = argparse.ArgumentParser(description="FLOW cleanup orchestrator")
    parser.add_argument("project_root", help="Path to project root")
    parser.add_argument("--branch", required=True, help="Branch name")
    parser.add_argument("--worktree", required=True, help="Worktree path (relative)")
    parser.add_argument("--pr", type=int, default=None, help="PR number to close")
    parser.add_argument("--delete-remote", action="store_true",
                        help="Delete remote and local branch (abort mode)")
    args = parser.parse_args()

    root = Path(args.project_root)
    if not root.is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Project root not found: {args.project_root}",
        }))
        sys.exit(1)

    steps = cleanup(root, args.branch, args.worktree, args.pr, args.delete_remote)
    print(json.dumps({"status": "ok", "steps": steps}))


if __name__ == "__main__":
    main()
