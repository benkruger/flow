"""Bridge: delegates to Rust via bin/flow log.

Usage: bin/flow log <branch> "<message>"

This module is a thin bridge that preserves the Python import interface
for init-state.py and start-setup.py. The actual implementation lives
in the Rust binary (src/commands/log.rs).

bin/flow is resolved relative to this file's location (the FLOW plugin
repo), not via project_root(). Target projects do not have bin/flow.
"""

import subprocess
import sys
from pathlib import Path

_PLUGIN_ROOT = Path(__file__).resolve().parent.parent
_BIN_FLOW = str(_PLUGIN_ROOT / "bin" / "flow")


def append_log(branch, message):
    """Append a timestamped message to the branch log file via Rust."""
    subprocess.run([_BIN_FLOW, "log", branch, message], check=False)


def main():
    if len(sys.argv) < 3:
        print("Usage: bin/flow log <branch> <message>")
        sys.exit(1)

    branch = sys.argv[1]
    message = sys.argv[2]
    append_log(branch, message)


if __name__ == "__main__":
    main()
