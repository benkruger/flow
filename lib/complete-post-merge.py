"""Consolidated post-merge operations for FLOW Complete phase.

Absorbs Steps 7 + 9 + 10: phase completion, PR body render, issues summary,
close issues, summary generation, label removal, auto-close parents, and
Slack notification. All operations are best-effort.

Usage: bin/flow complete-post-merge --pr <N> --state-file <path> --branch <name>

Output (JSON to stdout):
  {"status": "ok", "formatted_time": "...", "cumulative_seconds": N,
   "summary": "...", "issues_links": "...", "banner_line": "...",
   "closed_issues": [...], "parents_closed": [...],
   "slack": {"status": "ok"|"skipped"|"error", "ts": "..."},
   "failures": {...}}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT, NETWORK_TIMEOUT, mutate_state, project_root

BIN_FLOW = str(Path(__file__).resolve().parent.parent / "bin" / "flow")


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


def _parse_json(stdout, default=None):
    """Parse JSON from stdout. Returns (parsed, error_str)."""
    try:
        return json.loads(stdout.strip()), None
    except (json.JSONDecodeError, ValueError) as e:
        return default, str(e)


def post_merge(pr_number, state_file, branch, root=None):
    """Run all post-merge operations. Best-effort throughout.

    Args:
        pr_number: PR number.
        state_file: Path to state file.
        branch: Branch name.
        root: Project root path. Auto-detected if None.

    Returns a result dict with all outputs and a failures dict.
    """
    if root is None:
        root = project_root()
    else:
        root = Path(root)

    state_path = Path(state_file)
    failures = {}
    result = {
        "status": "ok",
        "formatted_time": "",
        "cumulative_seconds": 0,
        "summary": "",
        "issues_links": "",
        "banner_line": "",
        "closed_issues": [],
        "parents_closed": [],
        "slack": {"status": "skipped"},
        "failures": failures,
    }

    # Read state for slack_thread_ts and repo
    state = {}
    if state_path.exists():
        try:
            state = json.loads(state_path.read_text())
        except (json.JSONDecodeError, ValueError):
            pass

    repo = state.get("repo", "")

    # --- Step 7: Archive artifacts to PR ---

    # Set step counter
    if state_path.exists():

        def _set_step_7(s):
            s["complete_step"] = 6

        try:
            mutate_state(state_path, _set_step_7)
        except (json.JSONDecodeError, ValueError, FileNotFoundError):
            failures["step_counter"] = "could not update step counter"

    # Phase transition complete
    pt_result = _run_cmd(
        [
            BIN_FLOW,
            "phase-transition",
            "--phase",
            "flow-complete",
            "--action",
            "complete",
            "--next-phase",
            "flow-complete",
            "--branch",
            branch,
        ],
        timeout=NETWORK_TIMEOUT,
    )
    pt_data, pt_err = _parse_json(pt_result.stdout)
    if pt_data and pt_data.get("status") == "ok":
        result["formatted_time"] = pt_data.get("formatted_time", "")
        result["cumulative_seconds"] = pt_data.get("cumulative_seconds", 0)
    else:
        failures["phase_transition"] = pt_err or pt_result.stderr.strip()

    # Render PR body
    render_result = _run_cmd(
        [BIN_FLOW, "render-pr-body", "--pr", str(pr_number)],
        timeout=NETWORK_TIMEOUT,
    )
    if render_result.returncode != 0:
        failures["render_pr_body"] = render_result.stderr.strip()

    # Format issues summary
    issues_output = str(root / ".flow-states" / f"{branch}-issues.md")
    iss_result = _run_cmd(
        [BIN_FLOW, "format-issues-summary", "--state-file", state_file, "--output", issues_output],
    )
    iss_data, _ = _parse_json(iss_result.stdout)
    if iss_data and iss_data.get("has_issues"):
        result["banner_line"] = iss_data.get("banner_line", "")

    # --- Step 9: Close referenced issues ---

    close_result = _run_cmd(
        [BIN_FLOW, "close-issues", "--state-file", state_file],
        timeout=NETWORK_TIMEOUT,
    )
    close_data, _ = _parse_json(close_result.stdout)
    closed_issues = []
    if close_data:
        closed_issues = close_data.get("closed", [])
        result["closed_issues"] = closed_issues

    # Write closed-issues file if non-empty
    if closed_issues:
        closed_path = root / ".flow-states" / f"{branch}-closed-issues.json"
        try:
            closed_path.write_text(json.dumps(closed_issues))
        except OSError as e:
            failures["closed_issues_file"] = str(e)

    # --- Step 10: Parallel post-merge operations ---

    # Format complete summary
    summary_args = [BIN_FLOW, "format-complete-summary", "--state-file", state_file]
    if closed_issues:
        closed_file = str(root / ".flow-states" / f"{branch}-closed-issues.json")
        summary_args.extend(["--closed-issues-file", closed_file])
    sum_result = _run_cmd(summary_args)
    sum_data, _ = _parse_json(sum_result.stdout)
    if sum_data and sum_data.get("status") == "ok":
        result["summary"] = sum_data.get("summary", "")
        result["issues_links"] = sum_data.get("issues_links", "")

    # Remove In-Progress labels
    label_result = _run_cmd(
        [BIN_FLOW, "label-issues", "--state-file", state_file, "--remove"],
        timeout=NETWORK_TIMEOUT,
    )
    if label_result.returncode != 0:
        failures["label_issues"] = label_result.stderr.strip()

    # Auto-close parent issues for each closed issue
    for issue in closed_issues:
        issue_num = issue.get("number")
        if issue_num and repo:
            acp_result = _run_cmd(
                [BIN_FLOW, "auto-close-parent", "--repo", repo, "--issue-number", str(issue_num)],
                timeout=NETWORK_TIMEOUT,
            )
            acp_data, _ = _parse_json(acp_result.stdout)
            if acp_data and (acp_data.get("parent_closed") or acp_data.get("milestone_closed")):
                result["parents_closed"].append(issue_num)

    # Slack notification
    slack_thread_ts = state.get("slack_thread_ts")
    if slack_thread_ts:
        slack_result = _run_cmd(
            [
                BIN_FLOW,
                "notify-slack",
                "--phase",
                "flow-complete",
                "--message",
                f"Phase 6: Complete finished for PR #{pr_number}",
                "--thread-ts",
                slack_thread_ts,
            ],
            timeout=NETWORK_TIMEOUT,
        )
        slack_data, _ = _parse_json(slack_result.stdout)
        if slack_data:
            result["slack"] = slack_data
            # Record notification if successful
            if slack_data.get("status") == "ok" and slack_data.get("ts"):
                _run_cmd(
                    [
                        BIN_FLOW,
                        "add-notification",
                        "--phase",
                        "flow-complete",
                        "--ts",
                        slack_data["ts"],
                        "--thread-ts",
                        slack_thread_ts,
                        "--message",
                        f"Phase 6: Complete finished for PR #{pr_number}",
                    ],
                )
        else:
            result["slack"] = {"status": "error", "message": "invalid slack response"}

    return result


def main():
    parser = argparse.ArgumentParser(description="FLOW Complete phase post-merge operations")
    parser.add_argument("--pr", type=int, required=True, help="PR number")
    parser.add_argument("--state-file", required=True, help="Path to state file")
    parser.add_argument("--branch", required=True, help="Branch name")
    args = parser.parse_args()

    output = post_merge(pr_number=args.pr, state_file=args.state_file, branch=args.branch)
    print(json.dumps(output))


if __name__ == "__main__":
    main()
