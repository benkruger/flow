"""Tests for hooks/extract-release-notes.py."""

import importlib.util
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import HOOKS_DIR

# Import the hyphenated module directly
_spec = importlib.util.spec_from_file_location(
    "extract_release_notes", HOOKS_DIR / "extract-release-notes.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
extract = _mod.extract

SCRIPT = str(HOOKS_DIR / "extract-release-notes.py")

SAMPLE_NOTES = """\
# Release Notes

## v0.3.0 — Third release

- Feature C

---

## v0.2.0 — Second release

- Feature B
- Fix B

---

## v0.1.0 — Initial Release

- Feature A
"""


@pytest.fixture
def notes_file(tmp_path):
    p = tmp_path / "RELEASE-NOTES.md"
    p.write_text(SAMPLE_NOTES)
    return p


def test_extract_middle_version(notes_file):
    result = extract("v0.2.0", notes_file)
    assert result.startswith("## v0.2.0")
    assert "Feature B" in result
    assert "Feature A" not in result
    assert "Feature C" not in result


def test_extract_first_version(notes_file):
    result = extract("v0.3.0", notes_file)
    assert result.startswith("## v0.3.0")
    assert "Feature C" in result


def test_extract_last_version(notes_file):
    result = extract("v0.1.0", notes_file)
    assert result.startswith("## v0.1.0")
    assert "Feature A" in result


def test_missing_version_returns_empty(notes_file):
    result = extract("v9.9.9", notes_file)
    assert result == ""


def test_cli_writes_output_file(tmp_path):
    notes = tmp_path / "RELEASE-NOTES.md"
    notes.write_text(SAMPLE_NOTES)
    # Run from the parent of hooks/ so the script finds RELEASE-NOTES.md
    # But the script uses Path(__file__).parent.parent, so we need to
    # supply the file via the actual repo. Instead, test via subprocess
    # pointing at the real repo's RELEASE-NOTES.md.
    result = subprocess.run(
        [sys.executable, SCRIPT, "v0.5.1"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    out_file = Path("/tmp/release-notes-v0.5.1.md")
    assert out_file.exists()
    content = out_file.read_text()
    assert "v0.5.1" in content
    out_file.unlink()


def test_cli_missing_version_exits_1():
    result = subprocess.run(
        [sys.executable, SCRIPT, "v99.99.99"],
        capture_output=True, text=True,
    )
    assert result.returncode == 1
    assert "no section found" in result.stdout