"""Check GitHub for newer FLOW releases.

Usage: bin/flow upgrade-check

Output (JSON):
  {"status": "current", "installed": "0.18.0"}
  {"status": "upgrade_available", "installed": "0.18.0", "latest": "0.19.0"}
  {"status": "unknown", "reason": "..."}
"""

import json
import os
import subprocess
from pathlib import Path


def _parse_version(version_string):
    """Parse '0.18.0' into (0, 18, 0) for comparison."""
    return tuple(int(x) for x in version_string.split("."))


def main():
    plugin_json_override = os.environ.get("FLOW_PLUGIN_JSON")
    if plugin_json_override:
        plugin_json = Path(plugin_json_override)
    else:
        plugin_json = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    plugin_data = json.loads(plugin_json.read_text())
    installed = plugin_data["version"]

    repository = plugin_data.get("repository", "")
    prefix = "https://github.com/"
    if not repository.startswith(prefix):
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": "No GitHub repository URL in plugin.json",
                }
            )
        )
        return

    owner_repo = repository[len(prefix) :].rstrip("/")

    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{owner_repo}/releases/latest", "--jq", ".tag_name"],
            capture_output=True,
            text=True,
            timeout=int(os.environ.get("FLOW_UPGRADE_TIMEOUT", "10")),
        )
    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": "gh CLI not found",
                }
            )
        )
        return
    except subprocess.TimeoutExpired:
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": "GitHub API request timed out",
                }
            )
        )
        return

    if result.returncode != 0:
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": f"GitHub API request failed (exit {result.returncode})",
                }
            )
        )
        return

    tag = result.stdout.strip()
    if not tag:
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": "No releases found",
                }
            )
        )
        return

    latest = tag.lstrip("v")

    try:
        latest_tuple = _parse_version(latest)
        installed_tuple = _parse_version(installed)
    except (ValueError, AttributeError):
        print(
            json.dumps(
                {
                    "status": "unknown",
                    "reason": f"Could not parse version: {tag}",
                }
            )
        )
        return

    if latest_tuple > installed_tuple:
        print(
            json.dumps(
                {
                    "status": "upgrade_available",
                    "installed": installed,
                    "latest": latest,
                }
            )
        )
    else:
        print(
            json.dumps(
                {
                    "status": "current",
                    "installed": installed,
                }
            )
        )


if __name__ == "__main__":
    main()
