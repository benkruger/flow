"""Data-driven framework auto-detection.

Reads frameworks/*/detect.json to discover which frameworks match
a project based on file presence.

Usage: bin/flow detect-framework <project_root>

Output (JSON to stdout):
  {"status": "ok", "detected": [...], "available": [...]}
"""

import json
import sys
from pathlib import Path


def _frameworks_dir():
    """Return the frameworks/ directory relative to this script."""
    return Path(__file__).resolve().parent.parent / "frameworks"


def _load_detect_configs(frameworks_dir):
    """Load all detect.json files from frameworks/*/."""
    frameworks_path = Path(frameworks_dir)
    configs = []
    for detect_file in sorted(frameworks_path.glob("*/detect.json")):
        data = json.loads(detect_file.read_text())
        configs.append(data)
    return configs


def detect(project_root, frameworks_dir=None):
    """Return list of detected framework dicts for the given project root."""
    if frameworks_dir is None:
        frameworks_dir = str(_frameworks_dir())
    project = Path(project_root)
    configs = _load_detect_configs(frameworks_dir)
    detected = []
    for config in configs:
        for glob_pattern in config["detect_globs"]:
            if (project / glob_pattern).exists():
                detected.append({
                    "name": config["name"],
                    "display_name": config["display_name"],
                })
                break
    return detected


def available_frameworks(frameworks_dir=None):
    """Return list of all available framework dicts."""
    if frameworks_dir is None:
        frameworks_dir = str(_frameworks_dir())
    configs = _load_detect_configs(frameworks_dir)
    return [{"name": c["name"], "display_name": c["display_name"]} for c in configs]


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "message": "Usage: bin/flow detect-framework <project_root>",
        }))
        sys.exit(1)

    project_root = Path(sys.argv[1])
    if not project_root.is_dir():
        print(json.dumps({
            "status": "error",
            "message": f"Project root not found: {sys.argv[1]}",
        }))
        sys.exit(1)

    frameworks_dir = str(_frameworks_dir())
    detected = detect(str(project_root), frameworks_dir)
    available = available_frameworks(frameworks_dir)

    print(json.dumps({
        "status": "ok",
        "detected": detected,
        "available": available,
    }))


if __name__ == "__main__":
    main()
