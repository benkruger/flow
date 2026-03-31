"""Analyze open GitHub issues for the flow-issues skill.

Handles mechanical work: JSON parsing, file path extraction,
label detection, stale detection. Outputs condensed per-issue
briefs so the LLM only needs to rank by impact.

Usage:
  bin/flow analyze-issues                      # calls gh issue list internally
  bin/flow analyze-issues --issues-json <path> # reads pre-fetched JSON from file

Output (JSON to stdout):
  {"status": "ok", "total": N, "in_progress": [...], "issues": [...]}
"""

import argparse
import json
import os
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import detect_repo


def fetch_blocker_counts(repo, issue_numbers):
    """Fetch native blocked-by counts for issues via GitHub GraphQL API.

    Uses issueDependenciesSummary.blockedBy which returns the count of
    open blockers for each issue. Makes a single batched query using
    aliases to minimize API calls.

    Args:
        repo: "owner/repo" string.
        issue_numbers: List of issue numbers to query.

    Returns:
        Dict mapping issue number to blocker count. Empty dict on any
        failure (graceful degradation to label-only detection).
    """
    if not issue_numbers:
        return {}

    owner, name = repo.split("/", 1)

    # Build aliased query fragments for each issue
    fragments = []
    for number in issue_numbers:
        fragments.append(f"issue_{number}: issue(number: {number}) {{ issueDependenciesSummary {{ blockedBy }} }}")
    body = " ".join(fragments)
    query = f"query($owner: String!, $repo: String!) {{ repository(owner: $owner, name: $repo) {{ {body} }} }}"

    try:
        result = subprocess.run(
            [
                "gh",
                "api",
                "graphql",
                "-f",
                f"query={query}",
                "-f",
                f"owner={owner}",
                "-f",
                f"repo={name}",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return {}

    if result.returncode != 0:
        return {}

    try:
        data = json.loads(result.stdout)
    except (json.JSONDecodeError, TypeError):
        return {}

    repo_data = data.get("data", {}).get("repository", {})
    counts = {}
    for number in issue_numbers:
        issue_data = repo_data.get(f"issue_{number}", {})
        summary = issue_data.get("issueDependenciesSummary", {})
        counts[number] = summary.get("blockedBy", 0)

    return counts


# File path patterns: known directory prefixes or paths with file extensions
_DIR_PREFIXES = (
    "lib/",
    "skills/",
    "tests/",
    "docs/",
    "hooks/",
    "frameworks/",
    ".claude/",
    "bin/",
    "agents/",
    "src/",
    "config/",
    "app/",
)
_FILE_EXT_RE = re.compile(
    r"(?<!\w)"  # not preceded by word char
    r"([\w./\-]+/"  # at least one directory component
    r"[\w.\-]+"  # filename
    r"\.(?:py|md|json|sh|yml|yaml|rb|js|ts|html|css|toml))"  # extension
    r"(?!\w)"  # not followed by word char
)


def extract_file_paths(body):
    """Extract file paths from issue body text.

    Recognizes paths with known directory prefixes and paths containing
    slashes with recognized file extensions. Returns deduplicated list.
    """
    paths = set()

    # Match paths with known directory prefixes
    for prefix in _DIR_PREFIXES:
        escaped = re.escape(prefix)
        for match in re.finditer(escaped + r"[\w./\-]+", body):
            paths.add(match.group(0))

    # Match paths with file extensions (must contain /)
    for match in _FILE_EXT_RE.finditer(body):
        paths.add(match.group(1))

    return sorted(paths)


def detect_labels(labels):
    """Check for Flow In-Progress, Decomposed, and Blocked labels.

    Returns dict with in_progress, decomposed, and blocked boolean flags.
    """
    label_names = {label["name"] for label in labels}
    return {
        "in_progress": "Flow In-Progress" in label_names,
        "decomposed": any(name.lower() == "decomposed" for name in label_names),
        "blocked": any(name.lower() == "blocked" for name in label_names),
    }


_LABEL_CATEGORIES = {"Rule", "Flow", "Flaky Test", "Tech Debt", "Documentation Drift"}

_BUG_KEYWORDS = re.compile(r"\b(bug|fix|crash|error|broken|fail|wrong|incorrect)\b", re.IGNORECASE)
_ENHANCEMENT_KEYWORDS = re.compile(r"\b(add|new|feature|enhance|improve|support|implement)\b", re.IGNORECASE)


def categorize(label_names, title, body):
    """Assign a category based on label names first, then content fallback."""
    for label in _LABEL_CATEGORIES:
        if label in label_names:
            return label

    combined = f"{title} {body}"
    if _BUG_KEYWORDS.search(combined):
        return "Bug"
    if _ENHANCEMENT_KEYWORDS.search(combined):
        return "Enhancement"
    return "Other"


def check_stale(file_paths, age_days):
    """Check if an issue is stale (>60 days old with missing file refs).

    Returns dict with stale boolean and stale_missing count.
    """
    if age_days < 60 or not file_paths:
        return {"stale": False, "stale_missing": 0}

    missing = sum(1 for fp in file_paths if not os.path.exists(fp))
    return {"stale": missing > 0, "stale_missing": missing}


def truncate_body(body, max_length=200):
    """Truncate body to max_length, adding ellipsis if needed."""
    if len(body) <= max_length:
        return body
    return body[:max_length] + "..."


_FILTERS = {
    "ready": lambda i: not i["blocked"],
    "blocked": lambda i: i["blocked"],
    "decomposed": lambda i: i["decomposed"],
    "quick-start": lambda i: i["decomposed"] and not i["blocked"],
}


def filter_issues(issues, filter_name):
    """Filter analyzed issues by readiness criteria.

    Args:
        issues: List of analyzed issue dicts (from analyze_issues).
        filter_name: One of "ready", "blocked", "decomposed", "quick-start",
                     or None for no filtering.

    Returns:
        Filtered list of issues.

    Raises:
        ValueError: If filter_name is not recognized.
    """
    if filter_name is None:
        return issues
    if filter_name not in _FILTERS:
        raise ValueError(f"Unknown filter: {filter_name}")
    return [i for i in issues if _FILTERS[filter_name](i)]


def analyze_issues(issues, blocker_counts=None):
    """Analyze a list of issues from gh issue list JSON.

    Args:
        issues: List of issue dicts from gh issue list JSON.
        blocker_counts: Optional dict mapping issue number to native
            blocker count from GraphQL. When provided, an issue is
            blocked if it has the Blocked label OR native blocker
            count > 0.

    Returns structured result with in_progress and available issues.
    """
    if blocker_counts is None:
        blocker_counts = {}

    if not issues:
        return {"status": "ok", "total": 0, "in_progress": [], "issues": []}

    in_progress = []
    available = []

    for issue in issues:
        number = issue["number"]
        body = issue.get("body") or ""
        label_names = {label["name"] for label in issue.get("labels", [])}
        label_list = sorted(label_names)
        label_flags = detect_labels(issue.get("labels", []))

        if label_flags["in_progress"]:
            in_progress.append(
                {
                    "number": number,
                    "title": issue["title"],
                    "url": issue.get("url", ""),
                }
            )
            continue

        file_paths = extract_file_paths(body)

        created_at = datetime.fromisoformat(issue["createdAt"].replace("Z", "+00:00"))
        age_days = (datetime.now(created_at.tzinfo) - created_at).days

        stale_info = check_stale(file_paths, age_days)
        category = categorize(label_names, issue["title"], body)

        native_blocked = blocker_counts.get(number, 0) > 0

        available.append(
            {
                "number": number,
                "title": issue["title"],
                "url": issue.get("url", ""),
                "labels": label_list,
                "category": category,
                "age_days": age_days,
                "decomposed": label_flags["decomposed"],
                "blocked": label_flags["blocked"] or native_blocked,
                "native_blocked": native_blocked,
                "stale": stale_info["stale"],
                "stale_missing": stale_info["stale_missing"],
                "file_paths": file_paths,
                "brief": truncate_body(body),
            }
        )

    return {
        "status": "ok",
        "total": len(issues),
        "in_progress": in_progress,
        "issues": available,
    }


def main():
    parser = argparse.ArgumentParser(description="Analyze open GitHub issues")
    parser.add_argument(
        "--issues-json",
        help="Path to pre-fetched gh issue list JSON file (for testing)",
    )
    filter_group = parser.add_mutually_exclusive_group()
    filter_group.add_argument(
        "--ready",
        action="store_const",
        const="ready",
        dest="filter",
        help="Show only issues without Blocked label",
    )
    filter_group.add_argument(
        "--blocked",
        action="store_const",
        const="blocked",
        dest="filter",
        help="Show only issues with Blocked label",
    )
    filter_group.add_argument(
        "--decomposed",
        action="store_const",
        const="decomposed",
        dest="filter",
        help="Show only decomposed issues",
    )
    filter_group.add_argument(
        "--quick-start",
        action="store_const",
        const="quick-start",
        dest="filter",
        help="Show only decomposed issues without Blocked label",
    )
    args = parser.parse_args()

    if args.issues_json:
        try:
            raw = Path(args.issues_json).read_text()
        except Exception as exc:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": f"Could not read issues file: {exc}",
                    }
                )
            )
            sys.exit(1)
    else:
        try:
            result = subprocess.run(
                [
                    "gh",
                    "issue",
                    "list",
                    "--state",
                    "open",
                    "--json",
                    "number,title,labels,createdAt,body,url",
                    "--limit",
                    "100",
                ],
                capture_output=True,
                text=True,
                timeout=30,
            )
            if result.returncode != 0:
                print(
                    json.dumps(
                        {
                            "status": "error",
                            "message": f"gh issue list failed: {result.stderr.strip()}",
                        }
                    )
                )
                sys.exit(1)
            raw = result.stdout
        except subprocess.TimeoutExpired:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "message": "gh issue list timed out",
                    }
                )
            )
            sys.exit(1)

    try:
        issues = json.loads(raw)
    except json.JSONDecodeError as exc:
        print(
            json.dumps(
                {
                    "status": "error",
                    "message": f"Invalid JSON: {exc}",
                }
            )
        )
        sys.exit(1)

    # Fetch native blocker counts via GraphQL (best-effort)
    blocker_counts = {}
    repo = detect_repo()
    if repo:
        all_numbers = [i["number"] for i in issues]
        blocker_counts = fetch_blocker_counts(repo, all_numbers)

    output = analyze_issues(issues, blocker_counts=blocker_counts)
    if args.filter:
        output["issues"] = filter_issues(output["issues"], args.filter)
        output["total"] = len(output["in_progress"]) + len(output["issues"])
    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
