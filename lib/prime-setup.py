"""Consolidated setup for FLOW Prime.

Merges permissions into .claude/settings.json, writes .flow.json version
marker, and updates .git/info/exclude. Does NOT commit — the skill handles
git add + commit after this script runs.

Usage: bin/flow prime-setup <project_root>

Output (JSON to stdout):
  Success: {"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true}
  Failure: {"status": "error", "message": "..."}
"""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

from flow_utils import frameworks_dir as _frameworks_dir

UNIVERSAL_ALLOW = [
    "Bash(git -C *)",
    "Bash(git add *)",
    "Bash(git commit *)",
    "Bash(git push)",
    "Bash(git push -u *)",
    "Bash(git reset HEAD)",
    "Bash(git worktree *)",
    "Bash(git pull origin *)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(git push origin --delete *)",
    "Bash(git branch -D *)",
    "Bash(bin/ci)",
    "Bash(bin/dependencies)",
    "Bash(rm .flow-commit-*)",
    "Bash(rm .claude/settings.local.json)",
    "Bash(*bin/flow *)",
    "Bash(gh pr view *)",
    "Bash(git restore *)",
    "Bash(git fetch origin *)",
    "Bash(git merge *)",
    "Bash(gh pr checks *)",
    "Bash(gh pr merge *)",
    "Bash(claude plugin list)",
    "Bash(claude plugin marketplace add *)",
    "Bash(claude plugin install *)",
]

FLOW_DENY = [
    "Bash(git rebase *)",
    "Bash(git push --force *)",
    "Bash(git push -f *)",
    "Bash(git reset --hard *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git clean *)",
    "Bash(* && *)",
    "Bash(* ; *)",
    "Bash(* | *)",
]

EXCLUDE_ENTRIES = [".flow-states/", ".worktrees/", ".flow.json", "bin/dependencies"]


def _load_framework_permissions(framework):
    """Load permissions from frameworks/<name>/permissions.json."""
    permissions_path = _frameworks_dir() / framework / "permissions.json"
    if not permissions_path.exists():
        return []
    return json.loads(permissions_path.read_text())["allow"]


def _allow_list(framework):
    """Build the merged allow list for the given framework."""
    return UNIVERSAL_ALLOW + _load_framework_permissions(framework)


def compute_config_hash(framework):
    """Compute a deterministic hash of all structural config inputs.

    Hashes the canonical JSON of sorted allow list, deny list, exclude
    entries, and defaultMode. Returns a 12-char hex digest.
    """
    canonical = {
        "allow": sorted(_allow_list(framework)),
        "defaultMode": "acceptEdits",
        "deny": sorted(FLOW_DENY),
        "exclude": sorted(EXCLUDE_ENTRIES),
    }
    raw = json.dumps(canonical, sort_keys=True)
    return hashlib.sha256(raw.encode()).hexdigest()[:12]


def merge_settings(project_root, framework):
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
    for entry in _allow_list(framework):
        if entry not in existing_allow:
            settings["permissions"]["allow"].append(entry)

    existing_deny = set(settings["permissions"]["deny"])
    for entry in FLOW_DENY:
        if entry not in existing_deny:
            settings["permissions"]["deny"].append(entry)

    # Always set defaultMode to acceptEdits — FLOW requires it for state
    # file writes without permission prompts
    existing_mode = settings["permissions"].get("defaultMode")
    if existing_mode and existing_mode != "acceptEdits":
        print(
            f"Warning: Overriding defaultMode '{existing_mode}' with "
            f"'acceptEdits' — FLOW requires acceptEdits for state file writes",
            file=sys.stderr,
        )
    settings["permissions"]["defaultMode"] = "acceptEdits"

    # Write back
    settings_dir.mkdir(parents=True, exist_ok=True)
    settings_path.write_text(json.dumps(settings, indent=2) + "\n")

    return settings


def write_version_marker(project_root, version, framework, skills=None,
                         config_hash=None):
    """Write .flow.json with the plugin version, framework, and optional fields.

    If skills is provided, it is included as a top-level key mapping skill
    names to "auto" or "manual". If config_hash is provided, it is stored
    for version upgrade comparisons.
    """
    flow_json = project_root / ".flow.json"
    data = {"flow_version": version, "framework": framework}
    if config_hash is not None:
        data["config_hash"] = config_hash
    if skills is not None:
        data["skills"] = skills
    flow_json.write_text(json.dumps(data) + "\n")


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


def _plugin_json():
    """Read the full plugin.json as a dict."""
    plugin_path = (
        Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    )
    return json.loads(plugin_path.read_text())


def _plugin_version():
    """Read the current plugin version from plugin.json."""
    return _plugin_json()["version"]


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "message": "Usage: python3 prime-setup.py <project_root> --framework rails|python",
        }))
        sys.exit(1)

    project_root = Path(sys.argv[1])
    if not project_root.is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Project root not found: {sys.argv[1]}",
        }))
        sys.exit(1)

    # Parse --framework argument
    framework = None
    for i, arg in enumerate(sys.argv[2:], start=2):
        if arg == "--framework" and i + 1 < len(sys.argv):
            framework = sys.argv[i + 1]
            break

    if not framework or not (_frameworks_dir() / framework).is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Missing or invalid --framework argument: {framework}",
        }))
        sys.exit(1)

    try:
        plugin_data = _plugin_json()
        version = plugin_data["version"]
        config_hash = compute_config_hash(framework)
        merge_settings(project_root, framework)
        write_version_marker(project_root, version, framework,
                             config_hash=config_hash)
        exclude_updated = update_git_exclude(project_root)

        print(json.dumps({
            "status": "ok",
            "settings_merged": True,
            "exclude_updated": exclude_updated,
            "version_marker": True,
            "framework": framework,
        }))
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": str(e),
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
