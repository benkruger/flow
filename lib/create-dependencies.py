"""Create bin/dependencies from a framework template.

Copies frameworks/<name>/dependencies to the target project's
bin/dependencies. Skips if bin/dependencies already exists (user
may have customized it). Makes the file executable.

Usage: bin/flow create-dependencies <project_root> --framework <name>

Output (JSON to stdout):
  {"status": "ok", "path": "bin/dependencies"}
  {"status": "skipped", "message": "bin/dependencies already exists"}
"""

import json
import sys
from pathlib import Path

from flow_utils import frameworks_dir as _frameworks_dir


def create(project_root, framework, frameworks_dir=None):
    """Copy framework dependency template to bin/dependencies."""
    if frameworks_dir is None:
        frameworks_dir = str(_frameworks_dir())

    template_path = Path(frameworks_dir) / framework / "dependencies"
    if not template_path.exists():
        return {
            "status": "error",
            "message": f"Framework not found: {framework}",
        }

    project = Path(project_root)
    bin_dir = project / "bin"
    dependencies = bin_dir / "dependencies"

    if dependencies.exists():
        return {
            "status": "skipped",
            "message": "bin/dependencies already exists",
        }

    bin_dir.mkdir(parents=True, exist_ok=True)
    content = template_path.read_text()
    dependencies.write_text(content)
    dependencies.chmod(0o755)

    return {
        "status": "ok",
        "path": "bin/dependencies",
    }


def main():
    if len(sys.argv) < 2:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": "Usage: bin/flow create-dependencies <project_root> --framework <name>",
                }
            )
        )
        sys.exit(1)

    project_root = sys.argv[1]
    framework = None

    for i, arg in enumerate(sys.argv[2:], start=2):
        if arg == "--framework" and i + 1 < len(sys.argv):
            framework = sys.argv[i + 1]
            break

    if not framework:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": "Missing --framework argument",
                }
            )
        )
        sys.exit(1)

    result = create(project_root, framework)
    print(json.dumps(result))
    if result["status"] == "error":
        sys.exit(1)


if __name__ == "__main__":
    main()
