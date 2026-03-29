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
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from flow_utils import frameworks_dir as _frameworks_dir
from flow_utils import permission_to_regex


def _import_sibling(name, filename):
    """Import a sibling module with a hyphenated filename."""
    path = Path(__file__).resolve().parent / filename
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


UNIVERSAL_ALLOW = [
    "Bash(git add *)",
    "Bash(git blame *)",
    "Bash(git branch *)",
    "Bash(git commit *)",
    "Bash(git config *)",
    "Bash(git -C *)",
    "Bash(git diff *)",
    "Bash(git fetch *)",
    "Bash(git log *)",
    "Bash(git merge *)",
    "Bash(git pull *)",
    "Bash(git push)",
    "Bash(git push *)",
    "Bash(git remote *)",
    "Bash(git reset *)",
    "Bash(git restore *)",
    "Bash(git rev-list *)",
    "Bash(git rev-parse *)",
    "Bash(git show *)",
    "Bash(git status)",
    "Bash(git symbolic-ref *)",
    "Bash(git worktree *)",
    "Bash(cd *)",
    "Bash(pwd)",
    "Bash(chmod +x *)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(gh pr list *)",
    "Bash(gh pr view *)",
    "Bash(gh pr checks *)",
    "Bash(gh pr merge *)",
    "Bash(gh issue *)",
    "Bash(gh label *)",
    "Bash(gh -C *)",
    "Bash(*bin/*)",
    "Bash(rm .flow-*)",
    "Bash(rm tests/test_adversarial_*)",
    "Bash(claude plugin list)",
    "Bash(claude plugin marketplace add *)",
    "Bash(claude plugin install *)",
    "Bash(curl *)",
    "Read(~/.claude/rules/*)",
    "Read(~/.claude/projects/**/tool-results/*)",
    "Read(//tmp/*.txt)",
    "Read(//tmp/*.diff)",
    "Read(//tmp/*.patch)",
    "Read(//tmp/*.md)",
    "Agent(flow:ci-fixer)",
    "Skill(decompose:decompose)",
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

EXCLUDE_ENTRIES = [
    ".flow-states/",
    ".worktrees/",
    ".flow.json",
    "bin/dependencies",
    ".claude/scheduled_tasks.lock",
]


def _load_framework_permissions(framework):
    """Load permissions from frameworks/<name>/permissions.json."""
    permissions_path = _frameworks_dir() / framework / "permissions.json"
    if not permissions_path.exists():
        return []
    return json.loads(permissions_path.read_text())["allow"]


def _allow_list(framework):
    """Build the merged allow list for the given framework."""
    return UNIVERSAL_ALLOW + _load_framework_permissions(framework)


def _canonical_config(framework):
    """Build the canonical config dict for hashing.

    Args:
        framework: Framework name (e.g., 'python', 'rails')

    Returns:
        Dict with allow, defaultMode, deny, and exclude entries.
    """
    return {
        "allow": sorted(_allow_list(framework)),
        "defaultMode": "acceptEdits",
        "deny": sorted(FLOW_DENY),
        "exclude": sorted(EXCLUDE_ENTRIES),
    }


def compute_config_hash(framework):
    """Compute a deterministic hash of all structural config inputs.

    Hashes the canonical JSON of sorted allow list, deny list, exclude
    entries, and defaultMode. Returns a 12-char hex digest.

    Args:
        framework: Framework name (e.g., 'python', 'rails')
    """
    canonical = _canonical_config(framework)
    raw = json.dumps(canonical, sort_keys=True)
    return hashlib.sha256(raw.encode()).hexdigest()[:12]


def compute_setup_hash():
    """Compute a hash of the prime-setup.py file content.

    Any change to this file changes the hash and forces re-prime on
    next auto-upgrade. Returns a 12-char hex digest.
    """
    content = Path(__file__).resolve().read_bytes()
    return hashlib.sha256(content).hexdigest()[:12]


def _derive_permissions(project_root, framework):
    """Resolve derived permissions from frameworks/<name>/permissions.json.

    Reads the optional derived_permissions array. Each entry has a glob
    pattern and a template with {stem} placeholder. The glob is matched
    against the project root, and {stem} is replaced with the matched
    path's stem (filename without extension).

    Returns a list of resolved permission strings.
    """
    permissions_path = _frameworks_dir() / framework / "permissions.json"
    if not permissions_path.exists():
        return []
    data = json.loads(permissions_path.read_text())
    derived = data.get("derived_permissions", [])
    results = []
    for entry in derived:
        for match in sorted(Path(project_root).glob(entry["glob"])):
            results.append(entry["template"].replace("{stem}", match.stem))
            break
    return results


def _is_subsumed(candidate, existing_set):
    """Check if any entry in existing_set pattern-subsumes candidate.

    Uses permission_to_regex() to test whether an existing broader pattern
    (e.g. Bash(git *)) matches the candidate's concrete form (e.g. git add X).
    Only checks same-type entries (Bash vs Bash, not Agent vs Bash).
    """
    cand_match = re.match(r"(\w+)\((.+)\)", candidate)
    if not cand_match:
        return False
    cand_type, cand_inner = cand_match.group(1), cand_match.group(2)
    # Replace wildcards with literal text so regex tests structural coverage
    test_string = cand_inner.replace("*", "XXXPLACEHOLDERXXX")
    for existing in existing_set:
        if existing == candidate:
            continue
        ex_match = re.match(r"(\w+)\(", existing)
        if not ex_match or ex_match.group(1) != cand_type:
            continue
        regex = permission_to_regex(existing)
        if regex and regex.match(test_string):
            return True
    return False


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

    # Additive merge — only add entries not already present or subsumed
    existing_allow = set(settings["permissions"]["allow"])
    for entry in _allow_list(framework):
        if entry not in existing_allow and not _is_subsumed(entry, existing_allow):
            settings["permissions"]["allow"].append(entry)
            existing_allow.add(entry)

    # Merge derived permissions (project-specific, from glob detection)
    for entry in _derive_permissions(project_root, framework):
        if entry not in existing_allow and not _is_subsumed(entry, existing_allow):
            settings["permissions"]["allow"].append(entry)
            existing_allow.add(entry)

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


def write_version_marker(
    project_root,
    version,
    framework,
    skills=None,
    config_hash=None,
    setup_hash=None,
    commit_format=None,
    plugin_root=None,
):
    """Write .flow.json with the plugin version, framework, and optional fields.

    If skills is provided, it is included as a top-level key mapping skill
    names to "auto" or "manual". If config_hash is provided, it is stored
    for version upgrade comparisons. If setup_hash is provided, it is stored
    to detect changes to the setup script itself. If commit_format is
    provided, it is stored as a top-level key.
    """
    flow_json = project_root / ".flow.json"
    data = {"flow_version": version, "framework": framework}
    if config_hash is not None:
        data["config_hash"] = config_hash
    if setup_hash is not None:
        data["setup_hash"] = setup_hash
    if commit_format is not None:
        data["commit_format"] = commit_format
    if plugin_root is not None:
        data["plugin_root"] = plugin_root
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
            capture_output=True,
            text=True,
            check=True,
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
# Only enforce when the current branch has an active FLOW feature
branch=$(git symbolic-ref --short HEAD 2>/dev/null)
if [ -n "$branch" ] && [ -f ".flow-states/${branch}.json" ] && [ ! -f .flow-commit-msg ]; then
  echo "BLOCKED: FLOW feature in progress on ${branch}. Commits must go through /flow:flow-commit."
  echo "The file .flow-commit-msg was not found — this looks like a direct git commit."
  exit 1
fi
"""


def _install_script(directory, filename, content):
    """Create a directory, write a script file, and make it executable."""
    directory.mkdir(parents=True, exist_ok=True)
    target = directory / filename
    target.write_text(content)
    target.chmod(0o755)


def install_pre_commit_hook(project_root):
    """Install a pre-commit hook that blocks direct git commits.

    The hook checks for .flow-commit-msg — the fingerprint file created by
    /flow:flow-commit. If the file is missing, the commit is blocked.
    Idempotent: overwrites any existing pre-commit hook.
    """
    _install_script(project_root / ".git" / "hooks", "pre-commit", PRE_COMMIT_HOOK)


LAUNCHER_SCRIPT = """\
#!/usr/bin/env bash
# Global FLOW launcher — installed by /flow:flow-prime
# Reads plugin_root from .flow.json in the current git repo
set -euo pipefail

project_root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "Error: not inside a git repository" >&2
  exit 1
}

flow_json="$project_root/.flow.json"
if [ ! -f "$flow_json" ]; then
  echo "Error: $flow_json not found — run /flow:flow-prime in this project first" >&2
  exit 1
fi

plugin_root=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('plugin_root',''))" \
  "$flow_json" 2>/dev/null) || plugin_root=""
if [ -z "$plugin_root" ]; then
  echo "Error: plugin_root not found in $flow_json — run /flow:flow-prime to update" >&2
  exit 1
fi

if [ ! -d "$plugin_root" ]; then
  echo "Error: plugin path $plugin_root does not exist — run /flow:flow-prime to update" >&2
  exit 1
fi

exec "$plugin_root/bin/flow" "$@"
"""


def _home_dir():
    """Resolve the user's home directory, preferring $HOME for testability."""
    env_home = os.environ.get("HOME")
    return Path(env_home) if env_home else Path.home()


def install_launcher():
    """Install a global flow launcher at ~/.local/bin/flow.

    Creates ~/.local/bin/ if it does not exist. Idempotent: overwrites
    any existing launcher with the current version.
    """
    _install_script(_home_dir() / ".local" / "bin", "flow", LAUNCHER_SCRIPT)


def check_launcher_path():
    """Warn if ~/.local/bin is not in PATH."""
    local_bin = str(_home_dir() / ".local" / "bin")
    path_dirs = os.environ.get("PATH", "").split(os.pathsep)
    if local_bin not in path_dirs:
        print(
            f"Warning: {local_bin} is not in your PATH. "
            f"Add this to your shell profile:\n"
            f'  export PATH="$HOME/.local/bin:$PATH"',
            file=sys.stderr,
        )


def _plugin_json():
    """Read the full plugin.json as a dict."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())


def _plugin_version():
    """Read the current plugin version from plugin.json."""
    return _plugin_json()["version"]


def main():
    if len(sys.argv) < 2:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": "Usage: python3 prime-setup.py <project_root> --framework rails|python",
                }
            )
        )
        sys.exit(1)

    project_root = Path(sys.argv[1])
    if not project_root.is_dir():
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Project root not found: {sys.argv[1]}",
                }
            )
        )
        sys.exit(1)

    # Parse arguments
    framework = None
    skills_json = None
    commit_format = None
    plugin_root = None
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
        elif sys.argv[i] == "--plugin-root" and i + 1 < len(sys.argv):
            plugin_root = sys.argv[i + 1]
            i += 2
        else:
            i += 1

    if not framework or not (_frameworks_dir() / framework).is_dir():
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Missing or invalid --framework argument: {framework}",
                }
            )
        )
        sys.exit(1)

    skills = None
    if skills_json is not None:
        try:
            skills = json.loads(skills_json)
        except json.JSONDecodeError as e:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": f"Invalid --skills-json: {e}",
                    }
                )
            )
            sys.exit(1)

    try:
        plugin_data = _plugin_json()
        version = plugin_data["version"]
        config_hash = compute_config_hash(framework)
        setup_hash = compute_setup_hash()
        merge_settings(project_root, framework)
        write_version_marker(
            project_root,
            version,
            framework,
            skills=skills,
            config_hash=config_hash,
            setup_hash=setup_hash,
            commit_format=commit_format,
            plugin_root=plugin_root,
        )
        exclude_updated = update_git_exclude(project_root)
        install_pre_commit_hook(project_root)

        launcher_installed = False
        if plugin_root is not None:
            install_launcher()
            check_launcher_path()
            launcher_installed = True

        _prime_project = _import_sibling("prime_project", "prime-project.py")
        _create_deps = _import_sibling("create_deps", "create-dependencies.py")
        prime_result = _prime_project.prime(str(project_root), framework)
        deps_result = _create_deps.create(str(project_root), framework)

        print(
            json.dumps(
                {
                    "status": "ok",
                    "settings_merged": True,
                    "exclude_updated": exclude_updated,
                    "version_marker": True,
                    "hook_installed": True,
                    "launcher_installed": launcher_installed,
                    "framework": framework,
                    "prime_project": prime_result["status"],
                    "dependencies": deps_result["status"],
                }
            )
        )
    except Exception as e:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": str(e),
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
