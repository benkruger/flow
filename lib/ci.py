"""Run the target project's bin/ci with dirty-check optimization.

Usage:
  bin/flow ci [--force] [--retry N] [--simulate-branch <name>]

By default, skips if nothing changed since the last passing run.
With --force, always runs bin/ci regardless of sentinel state.
With --retry N, runs up to N times with force semantics. Classifies
failures as flaky (passes on retry) or consistent (all attempts fail).
With --simulate-branch, sets FLOW_SIMULATE_BRANCH in the child
environment so current_branch() returns the simulated name during
test execution. The simulated branch name is incorporated into the
sentinel snapshot hash so runs with different --simulate-branch
values produce distinct sentinels.

Output (JSON to stdout):
  Success:       {"status": "ok", "skipped": false}
  Skipped:       {"status": "ok", "skipped": true, "reason": "..."}
  Error:         {"status": "error", "message": "..."}
  Retry pass:    {"status": "ok", "attempts": 1}
  Retry flaky:   {"status": "ok", "attempts": 2, "flaky": true, "first_failure_output": "..."}
  Retry fail:    {"status": "error", "attempts": 3, "consistent": true, "output": "..."}
"""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from flow_utils import project_root, resolve_branch


def _tree_snapshot(root, simulate_branch=None):
    """Return a content-aware SHA-256 hash of the working tree state.

    Combines three signals into a single digest:
    1. HEAD commit hash — changes after every commit
    2. git diff HEAD — captures all tracked content changes (staged + unstaged)
    3. Untracked file content hashes via git hash-object — captures edits to
       untracked files that git status --porcelain would miss
    4. simulate_branch value (if provided) — ensures runs with different
       --simulate-branch values produce distinct hashes

    The old implementation used git status --porcelain which only captured
    file status (M, ??, A) without content. Editing an already-modified or
    untracked file produced an identical snapshot, causing incorrect skips.
    """
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    diff = subprocess.run(
        ["git", "diff", "HEAD"],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    untracked_hash = ""
    untracked_files = untracked.stdout.strip()
    if untracked_files:
        hash_result = subprocess.run(
            ["git", "hash-object", "--stdin-paths"],
            input=untracked_files,
            cwd=str(root),
            capture_output=True,
            text=True,
        )
        untracked_hash = hash_result.stdout

    combined = head.stdout.strip() + "\n" + diff.stdout + "\n" + untracked_files + "\n" + untracked_hash
    if simulate_branch is not None:
        combined += "\nsimulate:" + simulate_branch
    return hashlib.sha256(combined.encode()).hexdigest()


def _run_with_retry(bin_ci, cwd, sentinel, max_attempts, simulate_branch=None):
    """Run bin/ci up to max_attempts times, classifying as flaky or consistent."""
    first_failure_output = None

    for attempt in range(1, max_attempts + 1):
        result = subprocess.run(
            [str(bin_ci)],
            cwd=str(cwd),
            capture_output=True,
            text=True,
        )

        if result.returncode == 0:
            snapshot = _tree_snapshot(cwd, simulate_branch=simulate_branch)
            if sentinel:
                sentinel.parent.mkdir(parents=True, exist_ok=True)
                sentinel.write_text(snapshot)
            out = {"status": "ok", "attempts": attempt}
            if attempt > 1:
                out["flaky"] = True
                out["first_failure_output"] = first_failure_output
            print(json.dumps(out))
            sys.exit(0)
        else:
            if first_failure_output is None:
                first_failure_output = (result.stdout + result.stderr).strip()
            if sentinel and sentinel.exists():
                sentinel.unlink()

    # All attempts failed — consistent failure
    print(
        json.dumps(
            {
                "status": "error",
                "attempts": max_attempts,
                "consistent": True,
                "output": first_failure_output or "",
            }
        )
    )
    sys.exit(1)


def main():
    if os.environ.get("FLOW_CI_RUNNING"):
        print(json.dumps({"status": "ok", "skipped": True, "reason": "recursion guard"}))
        sys.exit(0)

    # Set guard immediately so child processes (bin/ci → pytest → bin/flow ci)
    # see it in their inherited environment and short-circuit.
    os.environ["FLOW_CI_RUNNING"] = "1"

    args = sys.argv[1:]
    force = "--force" in args

    cwd = Path.cwd()
    bin_ci = cwd / "bin" / "ci"
    root = project_root()

    # Extract --branch override from args
    branch_override = None
    if "--branch" in args:
        idx = args.index("--branch")
        if idx + 1 < len(args):
            branch_override = args[idx + 1]
            args = args[:idx] + args[idx + 2 :]

    # Extract --retry N from args
    retry = 0
    if "--retry" in args:
        idx = args.index("--retry")
        if idx + 1 < len(args):
            retry = int(args[idx + 1])
            args = args[:idx] + args[idx + 2 :]

    # Extract --simulate-branch from args (set in child env AND sentinel hash)
    simulate_branch = None
    if "--simulate-branch" in args:
        idx = args.index("--simulate-branch")
        if idx + 1 < len(args):
            simulate_branch = args[idx + 1]
            args = args[:idx] + args[idx + 2 :]

    branch, _ = resolve_branch(branch_override)
    sentinel = root / ".flow-states" / f"{branch}-ci-passed" if branch else None

    if not bin_ci.exists():
        print(json.dumps({"status": "error", "message": "bin/ci not found"}))
        sys.exit(1)

    # Set simulate-branch env var AFTER branch resolution (sentinel uses
    # the real branch) and BEFORE subprocess.run (child inherits it).
    if simulate_branch:
        os.environ["FLOW_SIMULATE_BRANCH"] = simulate_branch

    # Retry mode: run up to N times with force semantics
    if retry > 0:
        _run_with_retry(bin_ci, cwd, sentinel, retry, simulate_branch)

    snapshot = _tree_snapshot(cwd, simulate_branch=simulate_branch)

    if not force and sentinel and sentinel.exists():
        if sentinel.read_text() == snapshot:
            print(json.dumps({"status": "ok", "skipped": True, "reason": "no changes since last CI pass"}))
            sys.exit(0)

    result = subprocess.run(
        [str(bin_ci)],
        cwd=str(cwd),
    )

    if result.returncode == 0:
        if sentinel:
            sentinel.parent.mkdir(parents=True, exist_ok=True)
            sentinel.write_text(snapshot)
        print(json.dumps({"status": "ok", "skipped": False}))
        sys.exit(0)
    else:
        if sentinel and sentinel.exists():
            sentinel.unlink()
        print(json.dumps({"status": "error", "message": "bin/ci failed"}))
        sys.exit(1)


if __name__ == "__main__":
    main()
