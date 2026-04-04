"""Bridge module — exports fetch_database_id for Python callers.

The main issue creation logic has been ported to Rust (src/issue.rs).
This module retains only fetch_database_id, which is imported by
lib/create-sub-issue.py and lib/link-blocked-by.py.
"""

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import LOCAL_TIMEOUT


def fetch_database_id(repo, number):
    """Fetch the REST API database ID for an issue.

    The database ID is the integer ID used by REST API endpoints for
    sub-issues and dependencies. This is NOT the GraphQL node_id.

    Returns (id, error). id is an integer or None.
    """
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/issues/{number}", "--jq", ".id"],
            capture_output=True,
            text=True,
            timeout=LOCAL_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return None, f"gh api timed out after {LOCAL_TIMEOUT}s"

    if result.returncode != 0:
        error = result.stderr.strip() or "Unknown error"
        return None, error

    try:
        return int(result.stdout.strip()), None
    except (ValueError, TypeError):
        return None, f"Invalid ID from API: {result.stdout.strip()}"
