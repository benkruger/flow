"""Tests for lib/extract-release-notes.py."""

import importlib.util
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR, REPO_ROOT

# Import the hyphenated module directly
_spec = importlib.util.spec_from_file_location(
    "extract_release_notes", LIB_DIR / "extract-release-notes.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
extract = _mod.extract

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


def test_cli_writes_output_file(monkeypatch, capsys):
    monkeypatch.setattr("sys.argv", ["extract-release-notes.py", "v0.5.1"])

    _mod.main()

    captured = capsys.readouterr()
    out_file = REPO_ROOT / "tmp" / "release-notes-v0.5.1.md"
    assert out_file.exists()
    content = out_file.read_text()
    assert "v0.5.1" in content
    out_file.unlink()


def test_cli_no_arguments_exits_1(monkeypatch, capsys):
    """Running the script with no arguments should exit 1 with usage message."""
    monkeypatch.setattr("sys.argv", ["extract-release-notes.py"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "Usage" in capsys.readouterr().out


def test_version_in_body_text_not_matched(tmp_path):
    """A version string appearing in body text (not as a ## header) should not match."""
    notes = tmp_path / "RELEASE-NOTES.md"
    notes.write_text(
        "# Release Notes\n\n"
        "## v1.0.0 — First release\n\n"
        "- Upgraded from v0.9.0 to v1.0.0\n\n"
        "---\n\n"
        "## v0.8.0 — Earlier release\n\n"
        "- Feature\n"
    )
    result = extract("v0.9.0", notes)
    assert result == ""


def test_cli_missing_version_exits_1(monkeypatch, capsys):
    monkeypatch.setattr("sys.argv", ["extract-release-notes.py", "v99.99.99"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "no section found" in capsys.readouterr().out


def test_cli_invalid_version_format_exits_1(monkeypatch, capsys):
    """Running the script with a non-semver version should exit 1."""
    monkeypatch.setattr("sys.argv", ["extract-release-notes.py", "../../etc/passwd"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "invalid version format" in capsys.readouterr().out


# --- In-process test for exception path not reachable via subprocess ---


def test_main_file_not_found_exits_1(monkeypatch, tmp_path, capsys):
    """main() exits 1 when RELEASE-NOTES.md doesn't exist."""
    monkeypatch.setattr(sys, "argv", ["extract-release-notes.py", "v1.0.0"])
    monkeypatch.setattr(_mod, "__file__", str(tmp_path / "hooks" / "script.py"))
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "not found" in capsys.readouterr().out
