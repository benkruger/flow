"""Verify QA assertions per tier.

Usage: bin/flow qa-verify --tier <1|2|3> --framework <name> --repo <owner/repo>

Reads state files and GitHub state, checks per-step assertions,
outputs structured JSON pass/fail report.

Output (JSON to stdout):
  {"status": "ok", "tier": N, "checks": [{"name": "...", "passed": true/false, "detail": "..."}]}
  {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

REQUIRED_PHASES = [
    "flow-start", "flow-plan", "flow-code",
    "flow-code-review", "flow-learn", "flow-complete",
]


def _find_state_files(project_root):
    """Find all .flow-states/*.json files (excluding non-state files)."""
    state_dir = Path(project_root) / ".flow-states"
    if not state_dir.is_dir():
        return []
    return [
        f for f in state_dir.glob("*.json")
        if not f.name.startswith("orchestrate")
        and not f.name.endswith("-phases.json")
    ]


def _load_state(path):
    """Load and parse a state file."""
    try:
        return json.loads(Path(path).read_text())
    except (json.JSONDecodeError, FileNotFoundError):
        return None


def check_tier1(project_root, repo):
    """Tier 1: Single-flow full lifecycle verification.

    After a successful Complete phase, the state file is deleted, the
    worktree is removed, and the PR is merged. This checks post-Complete
    outcomes rather than mid-flow state.
    """
    checks = []

    # State files should be cleaned up after Complete
    state_files = _find_state_files(project_root)
    checks.append({
        "name": "State files cleaned up",
        "passed": len(state_files) == 0,
        "detail": "No leftover state files"
        if len(state_files) == 0
        else f"Found {len(state_files)} leftover state file(s)",
    })

    # Worktrees should be cleaned up after Complete
    worktrees_dir = Path(project_root) / ".worktrees"
    worktree_count = (
        len(list(worktrees_dir.iterdir())) if worktrees_dir.is_dir() else 0
    )
    checks.append({
        "name": "Worktrees cleaned up",
        "passed": worktree_count == 0,
        "detail": "No leftover worktrees"
        if worktree_count == 0
        else f"Found {worktree_count} leftover worktree(s)",
    })

    # At least one PR should be merged
    result = subprocess.run(
        ["gh", "pr", "list", "--repo", repo, "--state", "merged",
         "--limit", "1", "--json", "number"],
        capture_output=True, text=True,
    )
    if result.returncode == 0:
        pr_list = json.loads(result.stdout)
        has_merged = len(pr_list) >= 1
        detail = (
            f"PR #{pr_list[0]['number']} merged"
            if has_merged
            else "No merged PRs found"
        )
        checks.append({
            "name": "PR merged",
            "passed": has_merged,
            "detail": detail,
        })
    else:
        checks.append({
            "name": "PR merged",
            "passed": False,
            "detail": "Could not fetch merged PRs",
        })

    return {"status": "ok", "tier": 1, "checks": checks}


def check_tier2(project_root, repo):
    """Tier 2: Concurrent flow verification."""
    checks = []

    # Check at least 2 state files exist
    state_files = _find_state_files(project_root)
    checks.append({
        "name": "Two or more flows completed",
        "passed": len(state_files) >= 2,
        "detail": f"Found {len(state_files)} state file(s)",
    })

    if len(state_files) < 2:
        return {"status": "ok", "tier": 2, "checks": checks}

    # Load all states once
    loaded_states = [(sf, _load_state(sf)) for sf in state_files]

    # Check all flows have all phases complete
    all_complete = True
    for sf, state in loaded_states:
        if state is None:
            all_complete = False
            continue
        phases = state.get("phases", {})
        for p in REQUIRED_PHASES:
            if phases.get(p, {}).get("status") != "complete":
                all_complete = False
                break

    checks.append({
        "name": "All flows completed all phases",
        "passed": all_complete,
        "detail": f"Checked {len(state_files)} flows",
    })

    # Check branch isolation (different branches)
    branches = set()
    for sf, state in loaded_states:
        if state:
            branches.add(state.get("branch", ""))
    checks.append({
        "name": "Branch isolation",
        "passed": len(branches) == len(state_files),
        "detail": f"Unique branches: {len(branches)}",
    })

    return {"status": "ok", "tier": 2, "checks": checks}


def check_tier3(project_root, repo):
    """Tier 3: Stress and recovery verification."""
    checks = []

    # Check no stale lock file
    lock_file = Path(project_root) / ".flow-states" / "start.lock"
    if lock_file.exists():
        try:
            lock_data = json.loads(lock_file.read_text())
            checks.append({
                "name": "No stale lock",
                "passed": False,
                "detail": f"Lock held by feature: {lock_data.get('feature')}",
            })
        except (json.JSONDecodeError, OSError):
            checks.append({
                "name": "No stale lock",
                "passed": False,
                "detail": "Corrupt lock file exists",
            })
    else:
        checks.append({
            "name": "No stale lock",
            "passed": True,
            "detail": "No lock file present",
        })

    # Check no orphan state files (state files without matching worktrees)
    state_files = _find_state_files(project_root)
    orphans = []
    for sf in state_files:
        state = _load_state(sf)
        if state:
            branch = state.get("branch", "")
            wt_path = Path(project_root) / ".worktrees" / branch
            if not wt_path.exists():
                orphans.append(branch)

    checks.append({
        "name": "No orphan state files",
        "passed": len(orphans) == 0,
        "detail": f"Orphans: {orphans}" if orphans else "No orphans",
    })

    return {"status": "ok", "tier": 3, "checks": checks}


def verify(tier, framework, repo, project_root):
    """Dispatch to the correct tier check."""
    if tier == 1:
        return check_tier1(project_root, repo)
    elif tier == 2:
        return check_tier2(project_root, repo)
    elif tier == 3:
        return check_tier3(project_root, repo)
    else:
        return {"status": "error", "message": f"Invalid tier: {tier}"}


def main():
    parser = argparse.ArgumentParser(description="Verify QA assertions")
    parser.add_argument("--tier", type=int, required=True,
                        help="Tier number (1, 2, or 3)")
    parser.add_argument("--framework", default=None,
                        help="Framework name (reserved for future use)")
    parser.add_argument("--repo", required=True,
                        help="GitHub repo (owner/name)")
    parser.add_argument("--project-root", default=".",
                        help="Project root path")
    args = parser.parse_args()

    result = verify(args.tier, args.framework, args.repo, args.project_root)
    print(json.dumps(result, indent=2))
    if result.get("status") == "error":
        sys.exit(1)


if __name__ == "__main__":
    main()
