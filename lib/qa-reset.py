"""Reset a QA repo to seed state.

Usage: bin/flow qa-reset --repo <owner/repo> [--local-path <path>]

Resets git to the seed tag, closes PRs, deletes remote branches,
recreates issues from .qa/issues.json template.

Output (JSON to stdout):
  {"status": "ok", "prs_closed": N, "branches_deleted": N, "issues_reset": N}
  {"status": "error", "message": "..."}
"""

import argparse
import base64
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))


def reset_git(local_path):
    """Reset local repo to seed tag and force push."""
    commands = [
        ["git", "reset", "--hard", "seed"],
        ["git", "push", "-f", "origin", "main"],
    ]
    for cmd in commands:
        result = subprocess.run(
            cmd, capture_output=True, text=True, cwd=local_path,
        )
        if result.returncode != 0:
            return {
                "status": "error",
                "message": f"{' '.join(cmd[:3])} failed: "
                           f"{result.stderr.strip()}",
            }
    return {"status": "ok"}


def close_prs(repo):
    """Close all open PRs in the repo."""
    result = subprocess.run(
        ["gh", "pr", "list", "--repo", repo, "--state", "open",
         "--json", "number"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return 0

    prs = json.loads(result.stdout) if result.stdout.strip() else []
    closed = 0
    for pr in prs:
        r = subprocess.run(
            ["gh", "pr", "close", str(pr["number"]), "--repo", repo],
            capture_output=True, text=True,
        )
        if r.returncode == 0:
            closed += 1
    return closed


def delete_remote_branches(repo, local_path):
    """Delete all remote branches except main."""
    result = subprocess.run(
        ["git", "branch", "-r"],
        capture_output=True, text=True, cwd=local_path,
    )
    if result.returncode != 0:
        return 0

    branches = []
    for line in result.stdout.strip().split("\n"):
        branch = line.strip()
        if not branch:
            continue
        # Skip main and HEAD
        remote_name = branch.split("/", 1)[1] if "/" in branch else branch
        if remote_name in ("main", "HEAD -> origin/main"):
            continue
        branches.append(remote_name)

    deleted = 0
    for branch in branches:
        r = subprocess.run(
            ["git", "push", "origin", "--delete", branch],
            capture_output=True, text=True, cwd=local_path,
        )
        if r.returncode == 0:
            deleted += 1
    return deleted


def load_issue_template(repo):
    """Load the .qa/issues.json template from the repo."""
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/contents/.qa/issues.json",
         "--jq", ".content"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return []

    try:
        content = base64.b64decode(result.stdout.strip()).decode()
        return json.loads(content)
    except Exception:
        return []


def reset_issues(repo, template):
    """Close all existing issues and recreate from template."""
    # Close existing issues
    result = subprocess.run(
        ["gh", "issue", "list", "--repo", repo, "--state", "all",
         "--json", "number"],
        capture_output=True, text=True,
    )
    if result.returncode == 0 and result.stdout.strip():
        issues = json.loads(result.stdout)
        for issue in issues:
            subprocess.run(
                ["gh", "issue", "close", str(issue["number"]),
                 "--repo", repo],
                capture_output=True, text=True,
            )

    # Create new issues from template
    created = 0
    for issue in template:
        cmd = [
            "gh", "issue", "create",
            "--repo", repo,
            "--title", issue["title"],
            "--body", issue["body"],
        ]
        for label in issue.get("labels", []):
            cmd.extend(["--label", label])

        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode == 0:
            created += 1
    return created


def clean_local(local_path):
    """Remove FLOW artifacts from a local clone."""
    path = Path(local_path)
    for name in [".flow-states", ".claude"]:
        target = path / name
        if target.is_dir():
            shutil.rmtree(target)
    flow_json = path / ".flow.json"
    if flow_json.exists():
        flow_json.unlink()


def reset(repo, local_path=None):
    """Full reset workflow."""
    # If local_path provided, reset git first
    if local_path:
        git_result = reset_git(local_path)
        if git_result["status"] != "ok":
            return git_result

    prs_closed = close_prs(repo)
    branches_deleted = delete_remote_branches(
        repo, local_path or "."
    )

    template = load_issue_template(repo)
    issues_reset = reset_issues(repo, template)

    if local_path:
        clean_local(local_path)

    return {
        "status": "ok",
        "prs_closed": prs_closed,
        "branches_deleted": branches_deleted,
        "issues_reset": issues_reset,
    }


def main():
    parser = argparse.ArgumentParser(description="Reset a QA repo")
    parser.add_argument("--repo", required=True,
                        help="GitHub repo (owner/name)")
    parser.add_argument("--local-path", default=None,
                        help="Local clone path")
    args = parser.parse_args()

    result = reset(args.repo, local_path=args.local_path)
    print(json.dumps(result))
    if result["status"] != "ok":
        sys.exit(1)


if __name__ == "__main__":
    main()
