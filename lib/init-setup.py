"""Consolidated setup for FLOW Init.

Merges permissions into .claude/settings.json, writes .flow.json version
marker, and updates .git/info/exclude. Does NOT commit — the skill handles
git add + commit after this script runs.

Usage: bin/flow init-setup <project_root>

Output (JSON to stdout):
  Success: {"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true}
  Failure: {"status": "error", "message": "..."}
"""

import json
import subprocess
import sys
from pathlib import Path

FLOW_ALLOW = [
    "Bash(cd .worktrees/* && *)",
    "Bash(git add *)",
    "Bash(git commit *)",
    "Bash(git push)",
    "Bash(git push; *)",
    "Bash(git push -u *)",
    "Bash(git reset HEAD)",
    "Bash(git reset HEAD; *)",
    "Bash(git worktree *)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(git push origin --delete *)",
    "Bash(git branch -D *)",
    "Bash(bin/ci)",
    "Bash(bin/ci; *)",
    "Bash(bin/rails test *)",
    "Bash(rubocop *)",
    "Bash(rubocop -A)",
    "Bash(bundle update --all)",
    "Bash(bundle update --all; *)",
    "Bash(rm .flow-commit-*)",
    "Bash(bundle exec *)",
    "Bash(*bin/flow *)",
]

FLOW_DENY = [
    "Bash(git rebase *)",
    "Bash(git push --force *)",
    "Bash(git push -f *)",
    "Bash(git reset --hard *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git clean *)",
]

EXCLUDE_ENTRIES = [".flow-states/", ".worktrees/"]


def merge_settings(project_root):
    """Merge FLOW permissions into .claude/settings.json. Returns merged dict."""
    settings_dir = project_root / ".claude"
    settings_path = settings_dir / "settings.json"

    if settings_path.exists():
        settings = json.loads(settings_path.read_text())
    else:
        settings = {}

    # Ensure structure exists
    if "permissions" not in settings:
        settings["permissions"] = {}
    if "allow" not in settings["permissions"]:
        settings["permissions"]["allow"] = []
    if "deny" not in settings["permissions"]:
        settings["permissions"]["deny"] = []

    # Additive merge — only add entries not already present
    existing_allow = set(settings["permissions"]["allow"])
    for entry in FLOW_ALLOW:
        if entry not in existing_allow:
            settings["permissions"]["allow"].append(entry)

    existing_deny = set(settings["permissions"]["deny"])
    for entry in FLOW_DENY:
        if entry not in existing_deny:
            settings["permissions"]["deny"].append(entry)

    # Set defaultMode only if not already set
    if "defaultMode" not in settings["permissions"]:
        settings["permissions"]["defaultMode"] = "acceptEdits"

    # Write back
    settings_dir.mkdir(parents=True, exist_ok=True)
    settings_path.write_text(json.dumps(settings, indent=2))

    return settings


def write_version_marker(project_root, version):
    """Write .flow.json with the plugin version."""
    flow_json = project_root / ".flow.json"
    flow_json.write_text(json.dumps({"flow_version": version}))


def update_git_exclude(project_root):
    """Add .flow-states/ and .worktrees/ to .git/info/exclude if not present.

    Returns True if the file was updated, False if no changes needed.
    """
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--git-common-dir"],
            capture_output=True, text=True, check=True,
            cwd=str(project_root),
        )
        git_dir = Path(result.stdout.strip())
        if not git_dir.is_absolute():
            git_dir = project_root / git_dir
    except Exception:
        return False

    info_dir = git_dir / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    exclude_path = info_dir / "exclude"

    if exclude_path.exists():
        content = exclude_path.read_text()
    else:
        content = ""

    updated = False
    for entry in EXCLUDE_ENTRIES:
        if entry not in content:
            if content and not content.endswith("\n"):
                content += "\n"
            content += entry + "\n"
            updated = True

    if updated:
        exclude_path.write_text(content)

    return updated


def _plugin_version():
    """Read the current plugin version from plugin.json."""
    plugin_path = (
        Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    )
    return json.loads(plugin_path.read_text())["version"]


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "message": "Usage: python3 init-setup.py <project_root>",
        }))
        sys.exit(1)

    project_root = Path(sys.argv[1])
    if not project_root.is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Project root not found: {sys.argv[1]}",
        }))
        sys.exit(1)

    try:
        version = _plugin_version()
        merge_settings(project_root)
        write_version_marker(project_root, version)
        exclude_updated = update_git_exclude(project_root)

        print(json.dumps({
            "status": "ok",
            "settings_merged": True,
            "exclude_updated": exclude_updated,
            "version_marker": True,
        }))
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": str(e),
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
