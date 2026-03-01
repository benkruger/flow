"""Consolidated setup for FLOW Start phase.

Checks /flow:init version gate, runs git pull, creates worktree,
makes initial commit + push + PR, creates state file, and logs all operations.

Usage: bin/flow start-setup "<feature name>"

Output (JSON to stdout):
  Success: {"status": "ok", "worktree": "...", "pr_url": "...", "pr_number": N, "feature": "...", "branch": "..."}
  Failure: {"status": "error", "step": "...", "message": "..."}
"""

import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def _now():
    """Return current UTC timestamp in ISO 8601 format."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _branch_name(feature_words):
    """Convert feature words to a hyphenated branch name, max 32 chars."""
    name = "-".join(feature_words.lower().split())
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
    return " ".join(w.capitalize() for w in feature_words.split())


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
    return wt_path


def _initial_commit_push_pr(wt_path, branch, feature_title):
    """Make empty commit, push, and create PR. Returns (pr_url, pr_number)."""
    _run_cmd(
        ["git", "commit", "--allow-empty", "-m", f"Start {branch} branch"],
        wt_path, "commit",
    )
    _run_cmd(
        ["git", "push", "-u", "origin", branch],
        wt_path, "push",
    )

    pr_body = f"## What\\n\\n{feature_title}."
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
                       light_mode=False, framework="rails"):
    """Create the FLOW state file."""
    now = _now()
    phase_names = {
        1: "Start", 2: "Research", 3: "Design", 4: "Plan",
        5: "Code", 6: "Review", 7: "Security", 8: "Reflect", 9: "Cleanup",
    }
    phases = {}
    for i in range(1, 10):
        if i == 1:
            phases[str(i)] = {
                "name": phase_names[i],
                "status": "in_progress",
                "started_at": now,
                "completed_at": None,
                "session_started_at": now,
                "cumulative_seconds": 0,
                "visit_count": 1,
            }
        elif i == 3 and light_mode:
            phases[str(i)] = {
                "name": phase_names[i],
                "status": "complete",
                "started_at": None,
                "completed_at": None,
                "session_started_at": None,
                "cumulative_seconds": 0,
                "visit_count": 0,
                "skipped": True,
            }
        else:
            phases[str(i)] = {
                "name": phase_names[i],
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
        "started_at": now,
        "current_phase": 1,
        "framework": framework,
        "notes": [],
        "phases": phases,
    }
    if light_mode:
        state["mode"] = "light"

    state_dir = project_root / ".flow-states"
    state_dir.mkdir(parents=True, exist_ok=True)
    state_path = state_dir / f"{branch}.json"
    state_path.write_text(json.dumps(state, indent=2))
    return state


def _log(project_root, branch, message):
    """Append a log entry to .flow-states/<branch>.log."""
    log_dir = project_root / ".flow-states"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{branch}.log"
    timestamp = _now()
    with open(log_path, "a") as f:
        f.write(f"{timestamp} [Phase 1] {message}\n")


def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "status": "error",
            "step": "args",
            "message": "Feature name required. Usage: python3 start-setup.py \"<feature name>\"",
        }))
        sys.exit(1)

    feature_words = sys.argv[1]
    light_mode = "--light" in sys.argv[2:]
    branch = _branch_name(feature_words)
    feature_title = _title_case(feature_words)
    project_root = Path.cwd()

    try:
        # Version gate — ensure /flow:init has been run
        flow_json = project_root / ".flow.json"
        if not flow_json.exists():
            raise SetupError(
                "init_check",
                "FLOW not initialized. Run /flow:init first.",
            )
        init_data = json.loads(flow_json.read_text())
        plugin_version = json.loads(
            (Path(__file__).resolve().parent.parent
             / ".claude-plugin" / "plugin.json").read_text()
        )["version"]
        if init_data.get("flow_version") != plugin_version:
            raise SetupError(
                "init_check",
                f"FLOW version mismatch: initialized for "
                f"v{init_data.get('flow_version')}, plugin is "
                f"v{plugin_version}. Run /flow:init to upgrade.",
            )
        framework = init_data.get("framework")
        if framework not in ("rails", "python"):
            raise SetupError(
                "init_check",
                "Missing framework in .flow.json. Run /flow:init to configure.",
            )

        # Step 2a — Git pull
        _git_pull(project_root)
        _log(project_root, branch, "git pull origin main (exit 0)")

        # Step 2b — Create worktree
        wt_path = _create_worktree(project_root, branch)
        _log(project_root, branch, f"git worktree add .worktrees/{branch} (exit 0)")

        # Step 2e — Commit, push, PR
        pr_url, pr_number = _initial_commit_push_pr(wt_path, branch, feature_title)
        _log(project_root, branch, f"git commit + push + gh pr create (exit 0)")

        # Step 2f — Create state file
        _create_state_file(project_root, branch, feature_title, pr_url, pr_number,
                           light_mode=light_mode, framework=framework)
        _log(project_root, branch, f"create .flow-states/{branch}.json (exit 0)")

        output = {
            "status": "ok",
            "worktree": f".worktrees/{branch}",
            "pr_url": pr_url,
            "pr_number": pr_number,
            "feature": feature_title,
            "branch": branch,
        }
        if light_mode:
            output["mode"] = "light"
        print(json.dumps(output))

    except SetupError as e:
        print(json.dumps({
            "status": "error",
            "step": e.step,
            "message": e.message,
        }))


if __name__ == "__main__":
    main()