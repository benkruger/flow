"""Shared utilities for FLOW hooks.

Provides common functions used across multiple hook scripts:
- now: current Pacific Time timestamp
- format_time: human-readable time formatting
- project_root: find the main git repo root (works from worktrees)
- current_branch: get the current git branch name
"""

import fcntl
import hashlib
import json
import os
import re
import subprocess
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

PACIFIC = ZoneInfo("America/Los_Angeles")


def now():
    """Return current Pacific Time timestamp in ISO 8601 format."""
    return datetime.now(PACIFIC).isoformat(timespec="seconds")

_plugin_root = Path(__file__).resolve().parent.parent
_phases_json = _plugin_root / "flow-phases.json"
_config = json.loads(_phases_json.read_text())

PHASE_ORDER = _config["order"]
PHASE_NAMES = {key: _config["phases"][key]["name"] for key in PHASE_ORDER}
COMMANDS = {key: _config["phases"][key]["command"] for key in PHASE_ORDER}
PHASE_NUMBER = {key: i + 1 for i, key in enumerate(PHASE_ORDER)}


def load_phase_config(path):
    """Load phase config from a JSON file, returning (order, names, numbers, commands).

    Works with both the canonical flow-phases.json and frozen per-branch copies.
    """
    config = json.loads(Path(path).read_text())
    order = config["order"]
    names = {key: config["phases"][key]["name"] for key in order}
    commands = {key: config["phases"][key]["command"] for key in order}
    numbers = {key: i + 1 for i, key in enumerate(order)}
    return order, names, numbers, commands


def frameworks_dir():
    """Return the frameworks/ directory inside the plugin."""
    return _plugin_root / "frameworks"


def format_time(seconds):
    """Format seconds into human-readable time.

    Returns "Xh Ym" if >= 3600, "Xm" if >= 60, "<1m" if < 60.
    """
    if not isinstance(seconds, (int, float)):
        try:
            seconds = int(seconds)
        except (ValueError, TypeError):
            return "?"
    if seconds >= 3600:
        hours = seconds // 3600
        minutes = (seconds % 3600) // 60
        return f"{hours}h {minutes}m"
    if seconds >= 60:
        minutes = seconds // 60
        return f"{minutes}m"
    return "<1m"


def project_root():
    """Find the main git repository root.

    Uses `git worktree list --porcelain` to find the root, which works
    correctly whether run from the project root or from inside a worktree.
    Falls back to Path(".") if git fails.
    """
    try:
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            capture_output=True, text=True, check=True,
        )
        for line in result.stdout.strip().split("\n"):
            if line.startswith("worktree "):
                return Path(line.split(" ", 1)[1].strip())
    except Exception:
        pass
    return Path(".")


def current_branch():
    """Get the current git branch name.

    Returns None if not on a branch (e.g. detached HEAD) or if git fails.

    If FLOW_SIMULATE_BRANCH is set in the environment, returns that value
    instead of querying git. Used by ``bin/flow ci --simulate-branch`` to
    catch branch-dependent test failures before merge.
    """
    simulated = os.environ.get("FLOW_SIMULATE_BRANCH")
    if simulated:
        return simulated
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            capture_output=True, text=True, check=True,
        )
        return result.stdout.strip() or None
    except Exception:
        return None


def resolve_branch(override=None):
    """Resolve which branch's state file to use.

    Resolution order:
    1. If override provided, return it immediately
    2. If current_branch() matches a state file, return it
    3. Scan .flow-states/*.json (skip *-phases.json):
       - 1 file → return that branch (auto-resolve)
       - 2+ files → return (None, candidates) (ambiguous)
       - 0 files → return current_branch() (no features active)

    Returns (branch, candidates) where candidates is empty on success
    or a list of branch names when ambiguous.
    """
    if override:
        return (override, [])

    branch = current_branch()
    root = project_root()
    state_dir = root / ".flow-states"

    # Exact match — current branch has a state file
    if branch and (state_dir / f"{branch}.json").exists():
        return (branch, [])

    # Scan for state files
    if not state_dir.is_dir():
        return (branch, [])

    candidates = []
    for path in sorted(state_dir.glob("*.json")):
        if path.name.endswith("-phases.json"):
            continue
        try:
            json.loads(path.read_text())
            candidates.append(path.stem)
        except (json.JSONDecodeError, ValueError):
            continue

    if len(candidates) == 1:
        return (candidates[0], [])
    if len(candidates) > 1:
        return (None, candidates)

    # No state files found — return current branch (for new features)
    return (branch, [])


def find_state_files(root, branch):
    """Find state file(s), trying exact branch match first.

    Returns list of (Path, dict, str) tuples: (path, state, branch_name).
    Empty list = nothing found. Single item = unambiguous match.
    Multiple items = caller must disambiguate.
    """
    state_dir = root / ".flow-states"

    exact_path = state_dir / f"{branch}.json"
    if exact_path.exists():
        try:
            state = json.loads(exact_path.read_text())
            return [(exact_path, state, branch)]
        except (json.JSONDecodeError, ValueError):
            return []

    if not state_dir.is_dir():
        return []

    results = []
    for path in sorted(state_dir.glob("*.json")):
        if path.name.endswith("-phases.json"):
            continue
        try:
            state = json.loads(path.read_text())
            results.append((path, state, path.stem))
        except (json.JSONDecodeError, ValueError):
            continue

    return results


def extract_issue_numbers(prompt):
    """Extract unique issue numbers from #N patterns and GitHub URLs in a prompt string."""
    hash_matches = re.findall(r"#(\d+)", prompt)
    url_matches = re.findall(r"/issues/(\d+)", prompt)
    seen = set()
    result = []
    for match in hash_matches + url_matches:
        num = int(match)
        if num not in seen:
            seen.add(num)
            result.append(num)
    return result


def short_issue_ref(url):
    """Extract '#N' from a GitHub issue URL, falling back to the full URL."""
    match = re.search(r"/issues/(\d+)$", url)
    return f"#{match.group(1)}" if match else url


def derive_feature(branch):
    """Derive the human-readable feature name from a branch name.

    Title-cases each hyphen-separated word.
    """
    return " ".join(w.capitalize() for w in branch.replace("-", " ").split())


def derive_worktree(branch):
    """Derive the worktree path from a branch name."""
    return f".worktrees/{branch}"


def mutate_state(state_path, transform_fn):
    """Atomic read-lock-transform-write for state files.

    Opens the file, acquires an exclusive advisory lock, reads and parses
    JSON, calls transform_fn(state) to mutate the dict in place, then
    writes back and releases the lock.

    Returns the final (mutated) state dict.
    Raises json.JSONDecodeError on corrupt JSON, FileNotFoundError if missing.
    """
    with open(state_path, "r+") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        state = json.loads(f.read())
        transform_fn(state)
        f.seek(0)
        f.write(json.dumps(state, indent=2))
        f.truncate()
    return state


def detect_repo(cwd=None):
    """Auto-detect GitHub repo from git remote origin URL.

    Returns 'owner/repo' string or None if detection fails.
    Optional cwd parameter for running git in a specific directory.
    """
    try:
        result = subprocess.run(
            ["git", "remote", "get-url", "origin"],
            capture_output=True, text=True, cwd=cwd,
        )
        if result.returncode != 0:
            return None
        url = result.stdout.strip()
        if not url:
            return None
        match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$", url)
        if match:
            return match.group(1)
        return None
    except Exception:
        return None


def permission_to_regex(perm):
    """Convert a Bash(pattern) permission to a compiled regex.

    Bash(git push) -> ^git push$
    Bash(git push *) -> ^git push .*$
    Bash(bin/ci;*) -> ^bin/ci;.*$

    Returns None for non-Bash entries.
    """
    match = re.match(r"Bash\((.+)\)", perm)
    if not match:
        return None
    pattern = match.group(1)
    escaped = re.escape(pattern).replace(r"\*", ".*")
    return re.compile("^" + escaped + "$")


def elapsed_since(started_at, now=None):
    """Calculate elapsed seconds from an ISO timestamp to now."""
    if not started_at:
        return 0
    if now is None:
        now = datetime.now(PACIFIC)
    start = datetime.fromisoformat(started_at)
    return max(0, int((now - start).total_seconds()))


def read_version_from(plugin_json_path):
    """Read plugin version from a specific plugin.json path."""
    try:
        return json.loads(Path(plugin_json_path).read_text())["version"]
    except Exception:
        return "?"


def read_version():
    """Read plugin version from plugin.json next to this script."""
    plugin_json = _plugin_root / ".claude-plugin" / "plugin.json"
    return read_version_from(plugin_json)


def format_tab_title(state):
    """Format a terminal tab title from FLOW state.

    Returns a string like "#342 Feature Name — P3: Code (2)",
    or None if the state lacks required fields. Issue numbers from the prompt
    are prefixed to the feature name when present.
    """
    phase = state.get("current_phase")
    branch = state.get("branch")
    if not phase or not branch:
        return None

    number = PHASE_NUMBER.get(phase)
    name = PHASE_NAMES.get(phase)
    if number is None or name is None:
        return None

    step = ""
    if phase == "flow-code":
        task = state.get("code_task", 0)
        if isinstance(task, int) and task > 0:
            step = f" ({task})"
    elif phase == "flow-code-review":
        review_step = state.get("code_review_step", 0)
        if isinstance(review_step, int) and 0 < review_step < 4:
            step = f" ({review_step}/4)"

    feature = derive_feature(branch)
    issue_numbers = extract_issue_numbers(state.get("prompt", ""))
    if issue_numbers:
        issue_prefix = " ".join(f"#{n}" for n in issue_numbers)
        feature = f"{issue_prefix} {feature}"
    return f"{feature} \u2014 P{number}: {name}{step}"


TAB_COLORS = (
    (178, 34, 34),    # firebrick
    (0, 128, 128),    # teal
    (75, 0, 130),     # indigo
    (184, 134, 11),   # dark goldenrod
    (0, 100, 0),      # dark green
    (128, 0, 0),      # maroon
    (70, 130, 180),   # steel blue
    (139, 69, 19),    # saddle brown
    (72, 61, 139),    # dark slate blue
    (0, 139, 139),    # dark cyan
    (160, 82, 45),    # sienna
    (25, 25, 112),    # midnight blue
)


def format_tab_color(state=None, *, repo=None, override=None):
    """Return an (r, g, b) tuple for the terminal tab color.

    Hashes repo against TAB_COLORS for deterministic color.
    If override is a 3-element list/tuple, it is used instead.
    Returns None if repo is missing/empty and no valid override.

    repo can be provided directly via the keyword argument, or extracted
    from state["repo"]. The repo keyword takes precedence over state.
    """
    if isinstance(override, (list, tuple)) and len(override) == 3:
        return tuple(override)

    if repo is None and state is not None:
        repo = state.get("repo", "")
    if not repo:
        return None

    digest = hashlib.sha256(repo.encode()).digest()
    index = int.from_bytes(digest[:4], "big") % len(TAB_COLORS)
    return TAB_COLORS[index]
