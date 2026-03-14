"""Load suggested project permissions for a framework.

Reads the optional suggested_permissions array from
frameworks/<name>/permissions.json.

Usage: bin/flow load-suggestions <framework>

Output (JSON to stdout):
  {"status": "ok", "suggestions": [...]}
"""

import json
import sys
from pathlib import Path

from flow_utils import frameworks_dir as _frameworks_dir


def load(framework, frameworks_dir=None):
    """Return suggested permissions for the given framework.

    Returns a list of suggestion dicts, each with 'label' and 'template'.
    Returns an empty list if the framework has no suggestions or does not exist.
    """
    if frameworks_dir is None:
        frameworks_dir = str(_frameworks_dir())
    permissions_path = Path(frameworks_dir) / framework / "permissions.json"
    if not permissions_path.exists():
        return []
    data = json.loads(permissions_path.read_text())
    return data.get("suggested_permissions", [])


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "message": "Usage: bin/flow load-suggestions <framework>",
        }))
        sys.exit(1)

    framework = sys.argv[1]
    suggestions = load(framework)
    print(json.dumps({"status": "ok", "suggestions": suggestions}))


if __name__ == "__main__":
    main()
