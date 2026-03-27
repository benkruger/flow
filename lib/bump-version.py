#!/usr/bin/env python3
"""
Bump the FLOW plugin version across all files.

Updates plugin.json, marketplace.json, and all skill banners.

Usage: bin/flow bump-version <new_version>
Example: bin/flow bump-version 0.9.0
"""

import json
import re
import sys
from pathlib import Path


def validate_version(version: str) -> bool:
    return bool(re.match(r"^\d+\.\d+\.\d+$", version))


def read_current_version(plugin_json: Path) -> str:
    data = json.loads(plugin_json.read_text())
    return data["version"]


def bump_json(path: Path, old: str, new: str) -> bool:
    text = path.read_text()
    updated = text.replace(f'"version": "{old}"', f'"version": "{new}"')
    if updated == text:
        return False
    path.write_text(updated)
    return True


def bump_skill(path: Path, old: str, new: str) -> bool:
    text = path.read_text()
    updated = text.replace(f"FLOW v{old}", f"FLOW v{new}")
    if updated == text:
        return False
    path.write_text(updated)
    return True


def main():
    if len(sys.argv) != 2:
        print("Usage: bin/flow bump-version <new_version>")
        sys.exit(1)

    new_version = sys.argv[1]

    if not validate_version(new_version):
        print(f"Error: invalid version format: {new_version}")
        sys.exit(1)

    repo_root = Path(__file__).resolve().parent.parent
    plugin_json = repo_root / ".claude-plugin" / "plugin.json"

    if not plugin_json.exists():
        print(f"Error: {plugin_json} not found")
        sys.exit(1)

    old_version = read_current_version(plugin_json)

    if old_version == new_version:
        print(f"Error: version is already {new_version}")
        sys.exit(1)

    changed = []

    # 1. plugin.json
    if bump_json(plugin_json, old_version, new_version):
        changed.append(str(plugin_json.relative_to(repo_root)))

    # 2. marketplace.json
    marketplace_json = repo_root / ".claude-plugin" / "marketplace.json"
    if marketplace_json.exists() and bump_json(marketplace_json, old_version, new_version):
        changed.append(str(marketplace_json.relative_to(repo_root)))

    # 3. skills/*/SKILL.md
    skills_dir = repo_root / "skills"
    if skills_dir.exists():
        for skill_file in sorted(skills_dir.glob("*/SKILL.md")):
            if bump_skill(skill_file, old_version, new_version):
                changed.append(str(skill_file.relative_to(repo_root)))

    # 4. .claude/skills/flow-release/SKILL.md
    release_skill = repo_root / ".claude" / "skills" / "flow-release" / "SKILL.md"
    if release_skill.exists() and bump_skill(release_skill, old_version, new_version):
        changed.append(str(release_skill.relative_to(repo_root)))

    print(f"Bumped {old_version} -> {new_version}")
    print(f"Updated {len(changed)} files:")
    for f in changed:
        print(f"  {f}")


if __name__ == "__main__":
    main()
