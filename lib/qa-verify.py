"""Verify QA assertions after a completed flow.

Usage: bin/flow qa-verify --framework <name> --repo <owner/repo>

Checks post-Complete outcomes: cleanup (no leftover state files or
worktrees) and at least one merged PR.

Output (JSON to stdout):
  {"status": "ok", "checks": [{"name": "...", "passed": true/false, "detail": "..."}]}
"""

import argparse
import json
import subprocess
from pathlib import Path


def _find_state_files(project_root):
    """Find all .flow-states/*.json files (excluding non-state files)."""
    state_dir = Path(project_root) / ".flow-states"
    if not state_dir.is_dir():
        return []
    return [
        f
        for f in state_dir.glob("*.json")
        if not f.name.startswith("orchestrate") and not f.name.endswith("-phases.json")
    ]


def verify(framework, repo, project_root):
    """Verify post-Complete outcomes.

    After a successful Complete phase, the state file is deleted, the
    worktree is removed, and the PR is merged. This checks those outcomes.
    """
    checks = []

    # State files should be cleaned up after Complete
    state_files = _find_state_files(project_root)
    checks.append(
        {
            "name": "State files cleaned up",
            "passed": len(state_files) == 0,
            "detail": "No leftover state files"
            if len(state_files) == 0
            else f"Found {len(state_files)} leftover state file(s)",
        }
    )

    # Worktrees should be cleaned up after Complete
    worktrees_dir = Path(project_root) / ".worktrees"
    worktree_count = len(list(worktrees_dir.iterdir())) if worktrees_dir.is_dir() else 0
    checks.append(
        {
            "name": "Worktrees cleaned up",
            "passed": worktree_count == 0,
            "detail": "No leftover worktrees"
            if worktree_count == 0
            else f"Found {worktree_count} leftover worktree(s)",
        }
    )

    # At least one PR should be merged
    result = subprocess.run(
        ["gh", "pr", "list", "--repo", repo, "--state", "merged", "--limit", "1", "--json", "number"],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        pr_list = json.loads(result.stdout)
        has_merged = len(pr_list) >= 1
        detail = f"PR #{pr_list[0]['number']} merged" if has_merged else "No merged PRs found"
        checks.append(
            {
                "name": "PR merged",
                "passed": has_merged,
                "detail": detail,
            }
        )
    else:
        checks.append(
            {
                "name": "PR merged",
                "passed": False,
                "detail": "Could not fetch merged PRs",
            }
        )

    # At least one issue with the "decomposed" label should exist
    result = subprocess.run(
        ["gh", "issue", "list", "--repo", repo, "--label", "decomposed", "--state", "all", "--json", "number"],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        issue_list = json.loads(result.stdout)
        has_decomposed = len(issue_list) >= 1
        if has_decomposed:
            detail = f"{len(issue_list)} decomposed issue(s) found"
        else:
            detail = "No decomposed issues found"
        checks.append(
            {
                "name": "Decomposed issue created",
                "passed": has_decomposed,
                "detail": detail,
            }
        )
    else:
        checks.append(
            {
                "name": "Decomposed issue created",
                "passed": False,
                "detail": "Could not fetch decomposed issues",
            }
        )

    # No leftover .flow-issue-body-* files in project root
    body_files = list(Path(project_root).glob(".flow-issue-body-*"))
    checks.append(
        {
            "name": "No leftover body files",
            "passed": len(body_files) == 0,
            "detail": "No leftover body files"
            if len(body_files) == 0
            else f"Found {len(body_files)} leftover body file(s)",
        }
    )

    return {"status": "ok", "checks": checks}


def main():
    parser = argparse.ArgumentParser(description="Verify QA assertions")
    parser.add_argument("--framework", default=None, help="Framework name (reserved for future use)")
    parser.add_argument("--repo", required=True, help="GitHub repo (owner/name)")
    parser.add_argument("--project-root", default=".", help="Project root path")
    args = parser.parse_args()

    result = verify(args.framework, args.repo, args.project_root)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
