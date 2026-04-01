"""Version gate — verify /flow:flow-prime has been run with matching version.

Usage: bin/flow prime-check

Output (JSON to stdout):
  Success: {"status": "ok", "framework": "rails|python|ios|go|rust"}
  Auto-upgrade: {"status": "ok", "framework": "...", "auto_upgraded": true, "old_version": "...", "new_version": "..."}
  Failure: {"status": "error", "message": "..."}
"""

import importlib.util
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import read_flow_json


def _load_prime_setup():
    """Load the prime-setup module dynamically."""
    spec = importlib.util.spec_from_file_location(
        "prime_setup",
        Path(__file__).resolve().parent / "prime-setup.py",
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _compute_config_hash(framework):
    """Compute config hash via prime-setup module."""
    return _load_prime_setup().compute_config_hash(framework)


def _compute_setup_hash():
    """Compute setup hash via prime-setup module."""
    return _load_prime_setup().compute_setup_hash()


def _read_plugin_json():
    """Read the full plugin.json as a dict."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())


def main():
    project_root = Path.cwd()
    flow_json = project_root / ".flow.json"

    init_data = read_flow_json(project_root)
    if init_data is None:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": "FLOW not initialized. Run /flow:flow-prime first.",
                }
            )
        )
        return
    plugin_data = _read_plugin_json()
    plugin_version = plugin_data["version"]

    if init_data.get("flow_version") != plugin_version:
        stored_config = init_data.get("config_hash")
        stored_setup = init_data.get("setup_hash")
        framework = init_data.get("framework", "")
        plugin_config = _compute_config_hash(framework)
        plugin_setup = _compute_setup_hash()

        config_match = stored_config and plugin_config and stored_config == plugin_config
        setup_match = stored_setup and plugin_setup and stored_setup == plugin_setup

        if config_match and setup_match:
            old_version = init_data["flow_version"]
            init_data["flow_version"] = plugin_version
            flow_json.write_text(json.dumps(init_data) + "\n")

            print(
                json.dumps(
                    {
                        "status": "ok",
                        "framework": framework,
                        "auto_upgraded": True,
                        "old_version": old_version,
                        "new_version": plugin_version,
                    }
                )
            )
            return

        print(
            json.dumps(
                {
                    "status": "error",
                    "message": (
                        f"FLOW version mismatch: initialized for "
                        f"v{init_data.get('flow_version')}, plugin is "
                        f"v{plugin_version}. Run /flow:flow-prime --reprime to upgrade "
                        f"(keeps current config), or /flow:flow-prime to reconfigure."
                    ),
                }
            )
        )
        return

    framework = init_data.get("framework")
    if framework not in ("rails", "python", "ios", "go", "rust"):
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": "Missing framework in .flow.json. Run /flow:flow-prime to configure.",
                }
            )
        )
        return

    print(
        json.dumps(
            {
                "status": "ok",
                "framework": framework,
            }
        )
    )


if __name__ == "__main__":
    main()
