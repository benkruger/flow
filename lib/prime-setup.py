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

import importlib.util
from flow_utils import frameworks_dir as _frameworks_dir


def _import_sibling(name, filename):
    """Import a sibling module with a hyphenated filename."""
    path = Path(__file__).resolve().parent / filename
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod

UNIVERSAL_ALLOW = [
    "Bash(git -C *)",
    "Bash(git add *)",
    "Bash(git commit *)",
    "Bash(git push)",
    "Bash(git push -u *)",
    "Bash(git reset HEAD)",
    "Bash(git worktree *)",
    "Bash(git pull origin *)",
    "Bash(git status)",
    "Bash(git diff)",
    "Bash(git diff *)",
    "Bash(git log *)",
    "Bash(git branch --show-current)",
    "Bash(cd *)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(git push origin --delete *)",
    "Bash(git branch -D *)",
    "Bash(bin/*)",
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
    "Bash(gh issue list *)",
    "Bash(gh issue view *)",
    "Read(~/.claude/rules/*)",
    "Read(/tmp/*.txt)",
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

SETUP_EPOCH = 1


def _load_framework_permissions(framework):
    """Load permissions from frameworks/<name>/permissions.json."""
    permissions_path = _frameworks_dir() / framework / "permissions.json"
    if not permissions_path.exists():
        return []
    return json.loads(permissions_path.read_text())["allow"]


def _allow_list(framework):
    """Build the merged allow list for the given framework."""
    return UNIVERSAL_ALLOW + _load_framework_permissions(framework)


def _canonical_config(framework, setup_epoch=None):
    """Build the canonical config dict for hashing.

    Args:
        framework: Framework name (e.g., 'python', 'rails')
        setup_epoch: Override for SETUP_EPOCH (default: use current SETUP_EPOCH)

    Returns:
        Dict with allow, defaultMode, deny, exclude, and setup_epoch entries.
    """
    if setup_epoch is None:
        setup_epoch = SETUP_EPOCH
    return {
        "allow": sorted(_allow_list(framework)),
        "defaultMode": "acceptEdits",
        "deny": sorted(FLOW_DENY),
        "exclude": sorted(EXCLUDE_ENTRIES),
        "setup_epoch": setup_epoch,
    }


def compute_config_hash(framework, setup_epoch=None):
    """Compute a deterministic hash of all structural config inputs.

    Hashes the canonical JSON of sorted allow list, deny list, exclude
    entries, and defaultMode. Returns a 12-char hex digest.

    Args:
        framework: Framework name (e.g., 'python', 'rails')
        setup_epoch: Override for SETUP_EPOCH (default: use current SETUP_EPOCH)
    """
    canonical = _canonical_config(framework, setup_epoch)
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
                         config_hash=None, commit_format=None):
    """Write .flow.json with the plugin version, framework, and optional fields.

    If skills is provided, it is included as a top-level key mapping skill
    names to "auto" or "manual". If config_hash is provided, it is stored
    for version upgrade comparisons. If commit_format is provided, it is
    stored as a top-level key.
    """
    flow_json = project_root / ".flow.json"
    data = {"flow_version": version, "framework": framework}
    if config_hash is not None:
        data["config_hash"] = config_hash
    if commit_format is not None:
        data["commit_format"] = commit_format
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


PRE_COMMIT_HOOK = """\
#!/usr/bin/env bash
# .git/hooks/pre-commit — installed by /flow:flow-prime
if [ ! -f .flow-commit-msg ]; then
  echo "BLOCKED: Commits must go through /flow:flow-commit."
  echo "The file .flow-commit-msg was not found — this looks like a direct git commit."
  exit 1
fi
"""


def install_pre_commit_hook(project_root):
    """Install a pre-commit hook that blocks direct git commits.

    The hook checks for .flow-commit-msg — the fingerprint file created by
    /flow:flow-commit. If the file is missing, the commit is blocked.
    Idempotent: overwrites any existing pre-commit hook.
    """
    hooks_dir = project_root / ".git" / "hooks"
    hooks_dir.mkdir(parents=True, exist_ok=True)
    hook_path = hooks_dir / "pre-commit"
    hook_path.write_text(PRE_COMMIT_HOOK)
    hook_path.chmod(0o755)


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

    # Parse arguments
    framework = None
    skills_json = None
    commit_format = None
    i = 2
    while i < len(sys.argv):
        if sys.argv[i] == "--framework" and i + 1 < len(sys.argv):
            framework = sys.argv[i + 1]
            i += 2
        elif sys.argv[i] == "--skills-json" and i + 1 < len(sys.argv):
            skills_json = sys.argv[i + 1]
            i += 2
        elif sys.argv[i] == "--commit-format" and i + 1 < len(sys.argv):
            commit_format = sys.argv[i + 1]
            i += 2
        else:
            i += 1

    if not framework or not (_frameworks_dir() / framework).is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Missing or invalid --framework argument: {framework}",
        }))
        sys.exit(1)

    skills = None
    if skills_json is not None:
        try:
            skills = json.loads(skills_json)
        except json.JSONDecodeError as e:
            print(json.dumps({
                "status": "error",
                "message": f"Invalid --skills-json: {e}",
            }))
            sys.exit(1)

    try:
        plugin_data = _plugin_json()
        version = plugin_data["version"]
        config_hash = compute_config_hash(framework)
        merge_settings(project_root, framework)
        write_version_marker(project_root, version, framework,
                             skills=skills, config_hash=config_hash,
                             commit_format=commit_format)
        exclude_updated = update_git_exclude(project_root)
        install_pre_commit_hook(project_root)

        _prime_project = _import_sibling("prime_project", "prime-project.py")
        _create_deps = _import_sibling("create_deps", "create-dependencies.py")
        prime_result = _prime_project.prime(str(project_root), framework)
        deps_result = _create_deps.create(str(project_root), framework)

        print(json.dumps({
            "status": "ok",
            "settings_merged": True,
            "exclude_updated": exclude_updated,
            "version_marker": True,
            "hook_installed": True,
            "framework": framework,
            "prime_project": prime_result["status"],
            "dependencies": deps_result["status"],
        }))
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": str(e),
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
