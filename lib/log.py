"""Append a log entry to .flow-states/<branch>.log.

Usage: bin/flow log <branch> "<message>"

Appends a timestamped line to the log file. Creates the directory if needed.
No output on success, exit 1 on missing arguments.
"""

import fcntl
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, project_root


def append_log(branch, message):
    """Append a timestamped message to the branch log file."""
    root = project_root()
    log_dir = root / ".flow-states"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{branch}.log"
    timestamp = now()
    with open(log_path, "a") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        f.write(f"{timestamp} {message}\n")


def main():
    if len(sys.argv) < 3:
        print("Usage: bin/flow log <branch> <message>")
        sys.exit(1)

    branch = sys.argv[1]
    message = sys.argv[2]
    append_log(branch, message)


if __name__ == "__main__":
    main()
