"""Manage dev-mode plugin_root redirection in .flow.json.

Usage:
  bin/flow qa-mode --start --local-path <path>
  bin/flow qa-mode --start --local-path <path> --flow-json <path>
  bin/flow qa-mode --stop
  bin/flow qa-mode --stop --flow-json <path>

Start: saves current plugin_root as plugin_root_backup, overwrites
plugin_root with the local FLOW source path.

Stop: restores plugin_root from plugin_root_backup, removes the
backup key.

Output (JSON to stdout):
  Start OK:   {"status": "ok", "plugin_root": "...", "backup": "..."}
  Stop OK:    {"status": "ok", "restored": "..."}
  Error:      {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import project_root


def start(flow_json_path, local_source_path):
    """Redirect plugin_root to local source for dev testing.

    Reads .flow.json, validates preconditions, saves plugin_root as
    plugin_root_backup, overwrites plugin_root with local_source_path.

    Returns dict with status, plugin_root, and backup on success,
    or status and message on error.
    """
    flow_json_path = Path(flow_json_path)
    local_source_path = Path(local_source_path)

    if not flow_json_path.exists():
        return {"status": "error", "message": f".flow.json not found at {flow_json_path}"}

    data = json.loads(flow_json_path.read_text())

    if "plugin_root" not in data:
        return {
            "status": "error",
            "message": "plugin_root not found in .flow.json — run /flow:flow-prime first",
        }

    if "plugin_root_backup" in data:
        return {
            "status": "error",
            "message": "Already in dev mode — plugin_root_backup exists. Run --stop first.",
        }

    if not local_source_path.exists():
        return {
            "status": "error",
            "message": f"Local source path does not exist: {local_source_path}",
        }

    if not (local_source_path / "bin" / "flow").exists():
        return {
            "status": "error",
            "message": f"No bin/flow found in {local_source_path} — not a FLOW source directory",
        }

    backup = data["plugin_root"]
    data["plugin_root_backup"] = backup
    data["plugin_root"] = str(local_source_path)
    flow_json_path.write_text(json.dumps(data) + "\n")

    return {"status": "ok", "plugin_root": str(local_source_path), "backup": backup}


def stop(flow_json_path):
    """Restore plugin_root from backup after dev testing.

    Reads .flow.json, restores plugin_root from plugin_root_backup,
    removes the backup key.

    Returns dict with status and restored path on success,
    or status and message on error.
    """
    flow_json_path = Path(flow_json_path)

    if not flow_json_path.exists():
        return {"status": "error", "message": f".flow.json not found at {flow_json_path}"}

    data = json.loads(flow_json_path.read_text())

    if "plugin_root_backup" not in data:
        return {
            "status": "error",
            "message": "Not in dev mode — no plugin_root_backup found in .flow.json",
        }

    restored = data["plugin_root_backup"]
    data["plugin_root"] = restored
    del data["plugin_root_backup"]
    flow_json_path.write_text(json.dumps(data) + "\n")

    return {"status": "ok", "restored": restored}


def main():
    parser = argparse.ArgumentParser(description="Manage dev-mode plugin_root redirection")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--start", action="store_true", help="Switch to dev mode")
    group.add_argument("--stop", action="store_true", help="Switch back to marketplace mode")
    parser.add_argument("--local-path", type=str, help="Path to local FLOW source (required with --start)")
    parser.add_argument("--flow-json", type=str, help="Path to .flow.json (default: <project_root>/.flow.json)")
    args = parser.parse_args()

    if args.flow_json:
        flow_json_path = Path(args.flow_json)
    else:
        root = project_root()
        flow_json_path = root / ".flow.json"

    if args.start:
        if not args.local_path:
            print(json.dumps({
                "status": "error",
                "message": "--local-path is required with --start",
            }))
            sys.exit(1)
        result = start(flow_json_path, args.local_path)
    else:
        result = stop(flow_json_path)

    print(json.dumps(result))
    sys.exit(0 if result["status"] == "ok" else 1)


if __name__ == "__main__":
    main()
