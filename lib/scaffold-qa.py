"""Create a QA repo from per-framework templates.

Usage: bin/flow scaffold-qa --framework <name> --repo <owner/repo>

Reads template files from qa/templates/<framework>/, creates a GitHub repo,
writes the files, tags seed, and creates issues from .qa/issues.json.

Output (JSON to stdout):
  {"status": "ok", "repo": "...", "issues_created": N}
  {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))


def find_templates(framework, templates_dir=None):
    """Find all template files for a framework.

    Returns dict of {relative_path: content}.
    templates_dir defaults to qa/templates/ relative to this script's repo root.
    """
    if templates_dir is None:
        templates_dir = str(Path(__file__).resolve().parent.parent / "qa" / "templates")
    framework_dir = Path(templates_dir) / framework
    if not framework_dir.is_dir():
        raise ValueError(f"Unknown framework: {framework}")

    templates = {}
    for file_path in framework_dir.rglob("*"):
        if file_path.is_file():
            rel = str(file_path.relative_to(framework_dir))
            templates[rel] = file_path.read_text()
    return templates


def scaffold(framework, repo, templates_dir=None, clone_dir=None):
    """Create a QA repo from templates.

    1. gh repo create
    2. Write template files to clone_dir
    3. git init, add, commit, tag seed, push
    4. Create issues from .qa/issues.json
    """
    templates = find_templates(framework, templates_dir=templates_dir)

    # Create GitHub repo
    result = subprocess.run(
        ["gh", "repo", "create", repo, "--public", "--confirm"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return {
            "status": "error",
            "message": f"gh repo create failed: {result.stderr.strip()}",
        }

    # Set up clone directory
    if clone_dir is None:
        clone_dir = tempfile.mkdtemp(prefix="flow-qa-")
    clone_path = Path(clone_dir)
    if not clone_path.exists():
        clone_path.mkdir(parents=True)

    # Write template files
    issues_data = []
    for rel_path, content in templates.items():
        file_path = clone_path / rel_path
        file_path.parent.mkdir(parents=True, exist_ok=True)
        file_path.write_text(content)

        # Make bin scripts executable
        if rel_path.startswith("bin/"):
            file_path.chmod(0o755)

        # Extract issues data
        if rel_path == ".qa/issues.json":
            issues_data = json.loads(content)

    # Git init, add, commit, tag, push
    git_commands = [
        ["git", "init", "-b", "main"],
        ["git", "add", "-A"],
        ["git", "commit", "-m", "Initial commit"],
        ["git", "tag", "seed"],
        ["git", "remote", "add", "origin", f"https://github.com/{repo}.git"],
        ["git", "push", "-u", "origin", "main", "--tags"],
    ]
    for cmd in git_commands:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            cwd=str(clone_path),
        )
        if result.returncode != 0:
            return {
                "status": "error",
                "message": f"{' '.join(cmd[:3])} failed: {result.stderr.strip()}",
            }

    # Create issues from template
    issues_created = 0
    for issue in issues_data:
        cmd = [
            "gh",
            "issue",
            "create",
            "--repo",
            repo,
            "--title",
            issue["title"],
            "--body",
            issue["body"],
        ]
        for label in issue.get("labels", []):
            cmd.extend(["--label", label])

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            issues_created += 1

    return {
        "status": "ok",
        "repo": repo,
        "issues_created": issues_created,
    }


def main():
    parser = argparse.ArgumentParser(description="Create a QA repo")
    parser.add_argument("--framework", required=True, help="Framework name (rails, python, ios, go)")
    parser.add_argument("--repo", required=True, help="GitHub repo (owner/name)")
    args = parser.parse_args()

    result = scaffold(args.framework, args.repo)
    print(json.dumps(result))
    if result["status"] != "ok":
        sys.exit(1)


if __name__ == "__main__":
    main()
