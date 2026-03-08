"""Insert or replace FLOW priming content in a project's CLAUDE.md.

Reads frameworks/<name>/priming.md and inserts it between
<!-- FLOW:BEGIN --> / <!-- FLOW:END --> markers in the target
project's CLAUDE.md. Idempotent — re-running replaces existing
primed content.

Usage: bin/flow prime-project <project_root> --framework <name>

Output (JSON to stdout):
  {"status": "ok", "framework": "...", "project_root": "..."}
"""

import json
import sys
from pathlib import Path

from flow_utils import frameworks_dir as _frameworks_dir


MARKER_BEGIN = "<!-- FLOW:BEGIN -->"
MARKER_END = "<!-- FLOW:END -->"


def prime(project_root, framework, frameworks_dir=None):
    """Insert or replace priming content in CLAUDE.md."""
    if frameworks_dir is None:
        frameworks_dir = str(_frameworks_dir())

    project = Path(project_root)
    claude_md = project / "CLAUDE.md"

    if not claude_md.exists():
        return {
            "status": "error",
            "message": "CLAUDE.md not found in project root",
        }

    priming_path = Path(frameworks_dir) / framework / "priming.md"
    if not priming_path.exists():
        return {
            "status": "error",
            "message": f"Framework not found: {framework}",
        }

    priming_content = priming_path.read_text()
    existing_content = claude_md.read_text()

    block = f"{MARKER_BEGIN}\n{priming_content}\n{MARKER_END}\n"

    begin_index = existing_content.find(MARKER_BEGIN)
    end_index = existing_content.find(MARKER_END)
    if begin_index >= 0 and end_index >= 0:
        end_index += len(MARKER_END)
        if end_index < len(existing_content) and existing_content[end_index] == "\n":
            end_index += 1
        new_content = existing_content[:begin_index] + block + existing_content[end_index:]
    else:
        new_content = existing_content + "\n" + block

    claude_md.write_text(new_content)

    return {
        "status": "ok",
        "framework": framework,
        "project_root": str(project_root),
    }


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "message": "Usage: bin/flow prime-project <project_root> --framework <name>",
        }))
        sys.exit(1)

    project_root = sys.argv[1]
    framework = None

    for i, arg in enumerate(sys.argv[2:], start=2):
        if arg == "--framework" and i + 1 < len(sys.argv):
            framework = sys.argv[i + 1]
            break

    if not framework:
        print(json.dumps({
            "status": "error",
            "message": "Missing --framework argument",
        }))
        sys.exit(1)

    result = prime(project_root, framework)
    print(json.dumps(result))
    if result["status"] == "error":
        sys.exit(1)


if __name__ == "__main__":
    main()
