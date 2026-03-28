"""Promote permissions from settings.local.json into settings.json.

Reads .claude/settings.local.json, merges new permissions.allow entries
into .claude/settings.json, deletes settings.local.json, and outputs JSON.

Usage: bin/flow promote-permissions --worktree-path <path>

Output (JSON to stdout):
  {"status": "skipped", "reason": "no_local_file"}
  {"status": "ok", "promoted": [...], "already_present": N}
  {"status": "error", "message": "..."}
"""

import argparse
import json
import os
import sys


def promote_permissions(worktree_path):
    """Merge settings.local.json allow entries into settings.json.

    Returns a dict with status and details.
    """
    local_path = os.path.join(worktree_path, ".claude", "settings.local.json")
    settings_path = os.path.join(worktree_path, ".claude", "settings.json")

    if not os.path.exists(local_path):
        return {"status": "skipped", "reason": "no_local_file"}

    try:
        with open(local_path) as f:
            local_data = json.load(f)
    except (json.JSONDecodeError, OSError) as exc:
        return {"status": "error", "message": f"Could not parse settings.local.json: {exc}"}

    local_allow = local_data.get("permissions", {}).get("allow", [])

    if not os.path.exists(settings_path):
        return {"status": "error", "message": "settings.json does not exist"}

    try:
        with open(settings_path) as f:
            settings_data = json.load(f)
    except (json.JSONDecodeError, OSError) as exc:
        return {"status": "error", "message": f"Could not parse settings.json: {exc}"}

    existing_allow = settings_data.get("permissions", {}).get("allow", [])
    existing_set = set(existing_allow)

    promoted = []
    already_present = 0
    for entry in local_allow:
        if entry in existing_set:
            already_present += 1
        else:
            promoted.append(entry)
            existing_allow.append(entry)
            existing_set.add(entry)

    if "permissions" not in settings_data:
        settings_data["permissions"] = {}
    settings_data["permissions"]["allow"] = existing_allow

    try:
        with open(settings_path, "w") as f:
            json.dump(settings_data, f, indent=2)
            f.write("\n")
    except OSError as exc:
        return {"status": "error", "message": f"Could not write settings.json: {exc}"}

    try:
        os.remove(local_path)
    except OSError:
        pass

    return {"status": "ok", "promoted": promoted, "already_present": already_present}


def main():
    parser = argparse.ArgumentParser(description="Promote permissions from settings.local.json")
    parser.add_argument("--worktree-path", required=True, help="Path to worktree or project root")
    args = parser.parse_args()

    result = promote_permissions(args.worktree_path)
    print(json.dumps(result))

    if result["status"] == "error":
        sys.exit(1)


if __name__ == "__main__":
    main()
