"""Bridge: delegates to Rust via bin/flow log.

Usage: bin/flow log <branch> "<message>"

This module is a thin bridge that preserves the Python import interface
for init-state.py and start-setup.py. The actual implementation lives
in the Rust binary (src/commands/log.rs).

bin/flow is resolved relative to this file's location (the FLOW plugin
repo), not via project_root(). Target projects do not have bin/flow.

main() uses a direct Python fallback to avoid infinite recursion when
the Rust binary is not built — bin/flow would fall back to this script,
which would call bin/flow again.
"""

import subprocess
import sys
from pathlib import Path

_PLUGIN_ROOT = Path(__file__).resolve().parent.parent
_BIN_FLOW = str(_PLUGIN_ROOT / "bin" / "flow")


def append_log(branch, message):
    """Append a timestamped message to the branch log file via Rust."""
    subprocess.run([_BIN_FLOW, "log", branch, message], check=False)


def _direct_append(branch, message):
    """Fallback: append directly in Python when invoked via bin/flow dispatcher.

    Avoids infinite recursion: bin/flow -> python3 log.py -> bin/flow -> ...
    """
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from flow_utils import now, project_root

    root = project_root()
    log_dir = root / ".flow-states"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{branch}.log"
    timestamp = now()
    with open(log_path, "a") as f:
        import fcntl

        fcntl.flock(f, fcntl.LOCK_EX)
        f.write(f"{timestamp} {message}\n")


def main():
    if len(sys.argv) < 3:
        print("Usage: bin/flow log <branch> <message>")
        sys.exit(1)

    branch = sys.argv[1]
    message = sys.argv[2]
    _direct_append(branch, message)


if __name__ == "__main__":
    main()
