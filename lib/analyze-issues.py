"""Analyze open GitHub issues for the flow-issues skill.

Handles mechanical work: JSON parsing, file path extraction, dependency
detection, label detection, stale detection. Outputs condensed per-issue
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


# File path patterns: known directory prefixes or paths with file extensions
_DIR_PREFIXES = (
    "lib/", "skills/", "tests/", "docs/", "hooks/", "frameworks/",
    ".claude/", "bin/", "agents/", "src/", "config/", "app/",
)
_FILE_EXT_RE = re.compile(
    r"(?<!\w)"                       # not preceded by word char
    r"([\w./\-]+/"                   # at least one directory component
    r"[\w.\-]+"                      # filename
    r"\.(?:py|md|json|sh|yml|yaml|rb|js|ts|html|css|toml))"  # extension
    r"(?!\w)"                        # not followed by word char
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


def extract_dependencies(body, open_numbers, own_number=None):
    """Extract #N issue references that exist in the open set.

    Returns list of issue numbers this issue depends on.
    Excludes self-references.
    """
    matches = re.findall(r"#(\d+)", body)
    deps = []
    seen = set()
    for match in matches:
        num = int(match)
        if num in open_numbers and num not in seen and num != own_number:
            deps.append(num)
            seen.add(num)
    return deps


def detect_labels(labels):
    """Check for Flow In-Progress and Decomposed labels.

    Returns dict with in_progress and decomposed boolean flags.
    """
    label_names = {label["name"] for label in labels}
    return {
        "in_progress": "Flow In-Progress" in label_names,
        "decomposed": "Decomposed" in label_names,
    }


_LABEL_CATEGORIES = {
    "Rule": "Rule",
    "Flow": "Flow",
    "Flaky Test": "Flaky Test",
    "Tech Debt": "Tech Debt",
    "Documentation Drift": "Documentation Drift",
}

_BUG_KEYWORDS = re.compile(
    r"\b(bug|fix|crash|error|broken|fail|wrong|incorrect)\b", re.IGNORECASE
)
_ENHANCEMENT_KEYWORDS = re.compile(
    r"\b(add|new|feature|enhance|improve|support|implement)\b", re.IGNORECASE
)


def categorize(labels, title, body):
    """Assign a category based on labels first, then content fallback."""
    label_names = {label["name"] for label in labels}
    for label, category in _LABEL_CATEGORIES.items():
        if label in label_names:
            return category

    combined = f"{title} {body}"
    if _BUG_KEYWORDS.search(combined):
        return "Bug"
    if _ENHANCEMENT_KEYWORDS.search(combined):
        return "Enhancement"
    return "Other"


def check_stale(issue, file_paths, age_days):
    """Check if an issue is stale (>60 days old with missing file refs).

    Returns dict with stale boolean and stale_missing count.
    """
    if age_days < 60 or not file_paths:
        return {"stale": False, "stale_missing": 0}

    missing = sum(1 for fp in file_paths if not os.path.exists(fp))
    return {"stale": missing > 0, "stale_missing": missing}


def build_dependents(dependency_map):
    """Build reverse dependency map: who depends on each issue.

    Input: {issue_number: [dependency_numbers]}
    Output: {issue_number: [dependent_numbers]}
    """
    dependents = {}
    for issue_num, deps in dependency_map.items():
        for dep in deps:
            dependents.setdefault(dep, []).append(issue_num)
    return dependents


def truncate_body(body, max_length=200):
    """Truncate body to max_length, adding ellipsis if needed."""
    if len(body) <= max_length:
        return body
    return body[:max_length] + "..."


def analyze_issues(issues):
    """Analyze a list of issues from gh issue list JSON.

    Returns structured result with in_progress and available issues.
    """
    if not issues:
        return {"status": "ok", "total": 0, "in_progress": [], "issues": []}

    open_numbers = {issue["number"] for issue in issues}
    in_progress = []
    available = []

    # First pass: extract file paths and dependencies
    dependency_map = {}
    issue_data = {}

    for issue in issues:
        number = issue["number"]
        body = issue.get("body") or ""
        labels = issue.get("labels", [])
        label_flags = detect_labels(labels)
        file_paths = extract_file_paths(body)
        deps = extract_dependencies(body, open_numbers, own_number=number)
        dependency_map[number] = deps

        created_at = datetime.fromisoformat(issue["createdAt"].replace("Z", "+00:00"))
        age_days = (datetime.now(created_at.tzinfo) - created_at).days

        stale_info = check_stale(issue, file_paths, age_days)
        category = categorize(labels, issue["title"], body)

        label_names = [label["name"] for label in labels]

        issue_data[number] = {
            "number": number,
            "title": issue["title"],
            "url": issue.get("url", ""),
            "labels": label_names,
            "category": category,
            "age_days": age_days,
            "decomposed": label_flags["decomposed"],
            "stale": stale_info["stale"],
            "stale_missing": stale_info["stale_missing"],
            "dependencies": deps,
            "dependents": [],
            "file_paths": file_paths,
            "brief": truncate_body(body),
            "in_progress": label_flags["in_progress"],
        }

    # Second pass: build dependents
    dependents_map = build_dependents(dependency_map)
    for number, dependents in dependents_map.items():
        if number in issue_data:
            issue_data[number]["dependents"] = dependents

    # Separate in-progress from available
    for number, data in issue_data.items():
        if data["in_progress"]:
            in_progress.append({
                "number": data["number"],
                "title": data["title"],
                "url": data["url"],
            })
        else:
            entry = dict(data)
            del entry["in_progress"]
            available.append(entry)

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
    args = parser.parse_args()

    if args.issues_json:
        try:
            raw = Path(args.issues_json).read_text()
        except Exception as exc:
            print(json.dumps({
                "status": "error",
                "message": f"Could not read issues file: {exc}",
            }))
            sys.exit(1)
    else:
        try:
            result = subprocess.run(
                [
                    "gh", "issue", "list",
                    "--state", "open",
                    "--json", "number,title,labels,createdAt,body,url",
                    "--limit", "100",
                ],
                capture_output=True, text=True, timeout=30,
            )
            if result.returncode != 0:
                print(json.dumps({
                    "status": "error",
                    "message": f"gh issue list failed: {result.stderr.strip()}",
                }))
                sys.exit(1)
            raw = result.stdout
        except subprocess.TimeoutExpired:
            print(json.dumps({
                "status": "error",
                "message": "gh issue list timed out",
            }))
            sys.exit(1)

    try:
        issues = json.loads(raw)
    except json.JSONDecodeError as exc:
        print(json.dumps({
            "status": "error",
            "message": f"Invalid JSON: {exc}",
        }))
        sys.exit(1)

    output = analyze_issues(issues)
    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
