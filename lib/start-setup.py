"""Consolidated setup for FLOW Start phase.

Runs git pull, creates worktree, makes initial commit + push + PR,
creates state file, and logs all operations. The version gate
(prime-check) runs as a separate step before this script.

Usage: bin/flow start-setup "<feature name>" [--prompt "<full prompt>"]

Output (JSON to stdout):
  Success: {"status": "ok", "worktree": "...", "pr_url": "...", "pr_number": N, "feature": "...", "branch": "..."}
  Failure: {"status": "error", "step": "...", "message": "..."}
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, PHASE_NAMES, PHASE_NUMBER, PHASE_ORDER

PLUGIN_ROOT = Path(__file__).resolve().parent.parent


def _branch_name(feature_words):
    """Convert feature words to a hyphenated branch name, max 32 chars."""
    sanitized = re.sub(r"[^a-zA-Z0-9\s-]", "", feature_words)
    name = "-".join(sanitized.lower().split())
    if len(name) <= 32:
        return name
    # Truncate at last hyphen that fits within 32 chars
    truncated = name[:33]
    last_hyphen = truncated.rfind("-")
    if last_hyphen > 0:
        return truncated[:last_hyphen]
    return name[:32]


def _title_case(feature_words):
    """Title-case the feature name."""
    return " ".join(w.capitalize() for w in feature_words.replace("-", " ").split())


def _run_cmd(args, cwd, step_name):
    """Run a shell command, returning (stdout, stderr). Raises on failure."""
    result = subprocess.run(
        args, capture_output=True, text=True, cwd=str(cwd),
    )
    if result.returncode != 0:
        raise SetupError(step_name, result.stderr.strip() or result.stdout.strip())
    return result.stdout.strip(), result.stderr.strip()


class SetupError(Exception):
    """Error during setup with step identification."""

    def __init__(self, step, message):
        self.step = step
        self.message = message
        super().__init__(f"{step}: {message}")


def _git_pull(cwd):
    """Pull latest main."""
    _run_cmd(["git", "pull", "origin", "main"], cwd, "git_pull")


def _create_worktree(project_root, branch):
    """Create a git worktree at .worktrees/<branch>."""
    wt_path = project_root / ".worktrees" / branch
    _run_cmd(
        ["git", "worktree", "add", str(wt_path), "-b", branch],
        project_root, "worktree",
    )
    venv_dir = project_root / ".venv"
    if venv_dir.is_dir():
        (wt_path / ".venv").symlink_to(Path("..", "..", ".venv"))
    return wt_path


def _initial_commit_push_pr(wt_path, branch, feature_title):
    """Make empty commit, push, and create PR. Returns (pr_url, pr_number)."""
    commit_msg_path = wt_path / ".flow-commit-msg"
    try:
        commit_msg_path.write_text(f"Start {branch} branch")
        _run_cmd(
            ["git", "commit", "--allow-empty", "-F", ".flow-commit-msg"],
            wt_path, "commit",
        )
    finally:
        commit_msg_path.unlink(missing_ok=True)
    _run_cmd(
        ["git", "push", "-u", "origin", branch],
        wt_path, "push",
    )

    pr_body = f"## What\n\n{feature_title}."
    stdout, _ = _run_cmd(
        ["gh", "pr", "create",
         "--title", feature_title,
         "--body", pr_body,
         "--base", "main"],
        wt_path, "pr_create",
    )

    pr_url = stdout.strip()
    pr_number = _extract_pr_number(pr_url)
    return pr_url, pr_number


def _extract_pr_number(pr_url):
    """Extract PR number from URL like https://github.com/org/repo/pull/123."""
    parts = pr_url.rstrip("/").split("/")
    for i, part in enumerate(parts):
        if part == "pull" and i + 1 < len(parts):
            try:
                return int(parts[i + 1])
            except ValueError:
                pass
    return 0


def _create_state_file(project_root, branch, feature_title, pr_url, pr_number,
                       framework="rails", skills=None, prompt=""):
    """Create the FLOW state file."""
    current_time = now()
    phases = {}
    first_phase = PHASE_ORDER[0]
    for key in PHASE_ORDER:
        if key == first_phase:
            phases[key] = {
                "name": PHASE_NAMES[key],
                "status": "in_progress",
                "started_at": current_time,
                "completed_at": None,
                "session_started_at": current_time,
                "cumulative_seconds": 0,
                "visit_count": 1,
            }
        else:
            phases[key] = {
                "name": PHASE_NAMES[key],
                "status": "pending",
                "started_at": None,
                "completed_at": None,
                "session_started_at": None,
                "cumulative_seconds": 0,
                "visit_count": 0,
            }

    state = {
        "feature": feature_title,
        "branch": branch,
        "worktree": f".worktrees/{branch}",
        "pr_number": pr_number,
        "pr_url": pr_url,
        "started_at": current_time,
        "current_phase": "flow-start",
        "framework": framework,
        "files": {
            "plan": None,
            "dag": None,
            "log": f".flow-states/{branch}.log",
            "state": f".flow-states/{branch}.json",
        },
        "session_id": None,
        "transcript_path": None,
        "notes": [],
        "prompt": prompt,
        "phases": phases,
    }
    if skills is not None:
        state["skills"] = skills

    state_dir = project_root / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    state_path = state_dir / f"{branch}.json"
    state_path.write_text(json.dumps(state, indent=2))
    return state


def _freeze_phases(project_root, branch):
    """Copy flow-phases.json to .flow-states/<branch>-phases.json."""
    source = PLUGIN_ROOT / "flow-phases.json"
    dest_dir = project_root / ".flow-states"
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = dest_dir / f"{branch}-phases.json"
    shutil.copy2(source, dest)


def _log(project_root, branch, message):
    """Append a log entry to .flow-states/<branch>.log."""
    log_dir = project_root / ".flow-states"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{branch}.log"
    timestamp = now()
    with open(log_path, "a") as f:
        f.write(f"{timestamp} [Phase 1] {message}\n")


def main():
    parser = argparse.ArgumentParser(description="FLOW Start phase setup")
    parser.add_argument("feature_name", nargs="?", help="Feature name words")
    parser.add_argument("--prompt", default=None,
                        help="Full start prompt (preserved verbatim in state file)")
    args = parser.parse_args()

    if not args.feature_name:
        print(json.dumps({
            "status": "error",
            "step": "args",
            "message": "Feature name required. Usage: python3 start-setup.py \"<feature name>\"",
        }))
        sys.exit(1)

    feature_words = args.feature_name
    raw_prompt = args.prompt if args.prompt is not None else feature_words
    branch = _branch_name(feature_words)
    feature_title = _title_case(feature_words)
    project_root = Path.cwd()

    try:
        # Read framework from .flow.json (version gate already passed)
        flow_json = project_root / ".flow.json"
        init_data = json.loads(flow_json.read_text())
        framework = init_data.get("framework", "rails")
        skills = init_data.get("skills")

        # Git pull
        _git_pull(project_root)
        _log(project_root, branch, "git pull origin main (exit 0)")

        # Create worktree
        wt_path = _create_worktree(project_root, branch)
        _log(project_root, branch, f"git worktree add .worktrees/{branch} (exit 0)")

        # Commit, push, PR
        pr_url, pr_number = _initial_commit_push_pr(wt_path, branch, feature_title)
        _log(project_root, branch, f"git commit + push + gh pr create (exit 0)")

        # Create state file
        _create_state_file(project_root, branch, feature_title, pr_url, pr_number,
                           framework=framework, skills=skills, prompt=raw_prompt)
        _log(project_root, branch, f"create .flow-states/{branch}.json (exit 0)")

        # Freeze phase config for this feature
        _freeze_phases(project_root, branch)
        _log(project_root, branch, f"freeze .flow-states/{branch}-phases.json (exit 0)")

        output = {
            "status": "ok",
            "worktree": f".worktrees/{branch}",
            "pr_url": pr_url,
            "pr_number": pr_number,
            "feature": feature_title,
            "branch": branch,
        }
        print(json.dumps(output))

    except SetupError as e:
        print(json.dumps({
            "status": "error",
            "step": e.step,
            "message": e.message,
        }))


if __name__ == "__main__":
    main()