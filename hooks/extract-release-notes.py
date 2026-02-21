#!/usr/bin/env python3
"""
Extract release notes for a specific version from RELEASE-NOTES.md.
Writes the extracted section to /tmp/release-notes-<version>.md.

Usage: python3 hooks/extract-release-notes.py <version>
Example: python3 hooks/extract-release-notes.py v0.2.0
"""

import sys
from pathlib import Path


def extract(version: str, notes_file: Path) -> str:
    lines = notes_file.read_text().splitlines()
    section = []
    in_section = False

    for line in lines:
        if line.startswith("## ") and version in line:
            in_section = True
            section.append(line)
        elif line.startswith("## ") and in_section:
            break
        elif in_section:
            section.append(line)

    return "\n".join(section).strip()


def main():
    if len(sys.argv) != 2:
        print("Usage: python3 hooks/extract-release-notes.py <version>")
        sys.exit(1)

    version = sys.argv[1]
    notes_file = Path(__file__).parent.parent / "RELEASE-NOTES.md"

    if not notes_file.exists():
        print(f"Error: {notes_file} not found")
        sys.exit(1)

    content = extract(version, notes_file)

    if not content:
        print(f"Error: no section found for version {version}")
        sys.exit(1)

    out = Path(f"/tmp/release-notes-{version}.md")
    out.write_text(content + "\n")
    print(f"Written to {out}")


if __name__ == "__main__":
    main()
