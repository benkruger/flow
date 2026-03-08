"""Version gate — verify /flow:flow-init has been run with matching version.

Usage: bin/flow init-check

Output (JSON to stdout):
  Success: {"status": "ok", "framework": "rails|python"}
  Auto-upgrade: {"status": "ok", "framework": "...", "auto_upgraded": true}
  Failure: {"status": "error", "message": "..."}
"""

import json
import sys
from pathlib import Path


def _read_plugin_json():
    """Read the full plugin.json as a dict."""
    plugin_path = (
        Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    )
    return json.loads(plugin_path.read_text())


def main():
    project_root = Path.cwd()
    flow_json = project_root / ".flow.json"

    if not flow_json.exists():
        print(json.dumps({
            "status": "error",
            "message": "FLOW not initialized. Run /flow:flow-init first.",
        }))
        return

    init_data = json.loads(flow_json.read_text())
    plugin_data = _read_plugin_json()
    plugin_version = plugin_data["version"]

    if init_data.get("flow_version") != plugin_version:
        stored_hash = init_data.get("config_hash")
        framework = init_data.get("framework", "")
        plugin_hash = plugin_data.get("config_hash", {}).get(framework)

        if stored_hash and plugin_hash and stored_hash == plugin_hash:
            init_data["flow_version"] = plugin_version
            flow_json.write_text(json.dumps(init_data) + "\n")

            print(json.dumps({
                "status": "ok",
                "framework": framework,
                "auto_upgraded": True,
            }))
            return

        print(json.dumps({
            "status": "error",
            "message": (
                f"FLOW version mismatch: initialized for "
                f"v{init_data.get('flow_version')}, plugin is "
                f"v{plugin_version}. Run /flow:flow-init to upgrade."
            ),
        }))
        return

    framework = init_data.get("framework")
    if framework not in ("rails", "python"):
        print(json.dumps({
            "status": "error",
            "message": "Missing framework in .flow.json. Run /flow:flow-init to configure.",
        }))
        return

    print(json.dumps({
        "status": "ok",
        "framework": framework,
    }))


if __name__ == "__main__":
    main()
