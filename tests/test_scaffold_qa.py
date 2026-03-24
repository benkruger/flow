"""Tests for lib/scaffold-qa.py — create QA repos from templates."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import call, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("scaffold-qa")

REPO_ROOT = Path(__file__).resolve().parent.parent
TEMPLATES_DIR = REPO_ROOT / "qa" / "templates"


# --- find_templates ---


def test_find_templates_rails():
    """Finds all template files for rails framework."""
    templates = _mod.find_templates("rails", templates_dir=str(TEMPLATES_DIR))
    assert "Gemfile" in templates
    assert "bin/ci" in templates
    assert "app/models/calculator.rb" in templates
    assert "test/models/calculator_test.rb" in templates
    assert ".qa/issues.json" in templates


def test_find_templates_python():
    """Finds all template files for python framework."""
    templates = _mod.find_templates("python", templates_dir=str(TEMPLATES_DIR))
    assert "pyproject.toml" in templates
    assert "bin/ci" in templates
    assert "src/calculator.py" in templates
    assert "tests/test_calculator.py" in templates
    assert ".qa/issues.json" in templates


def test_find_templates_ios():
    """Finds all template files for ios framework."""
    templates = _mod.find_templates("ios", templates_dir=str(TEMPLATES_DIR))
    assert "FlowQA.xcodeproj/project.pbxproj" in templates
    assert "bin/ci" in templates
    assert "FlowQA/Calculator.swift" in templates
    assert "FlowQA/Secrets.swift.example" in templates
    assert "FlowQATests/CalculatorTests.swift" in templates
    assert "bin/test" in templates
    assert "bin/build" in templates
    assert ".qa/issues.json" in templates


def test_find_templates_unknown_framework():
    """Raises ValueError for unknown framework."""
    with pytest.raises(ValueError, match="Unknown framework"):
        _mod.find_templates("unknown", templates_dir=str(TEMPLATES_DIR))


def test_find_templates_preserves_content():
    """Template content matches the actual file on disk."""
    templates = _mod.find_templates("rails", templates_dir=str(TEMPLATES_DIR))
    actual = (TEMPLATES_DIR / "rails" / "Gemfile").read_text()
    assert templates["Gemfile"] == actual


# --- scaffold ---


def test_scaffold_creates_repo_and_issues(tmp_path):
    """scaffold() calls gh repo create, writes files, tags, and creates issues."""
    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run:
        mock_templates.return_value = {
            "Gemfile": "source 'https://rubygems.org'\n",
            "bin/ci": "#!/usr/bin/env ruby\nexit 0\n",
            ".qa/issues.json": json.dumps([
                {"title": "Issue 1", "body": "Body 1", "labels": []},
                {"title": "Issue 2", "body": "Body 2", "labels": ["bug"]},
            ]),
        }
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        result = _mod.scaffold("rails", "owner/flow-qa-rails",
                               clone_dir=str(tmp_path / "clone"))

    assert result["status"] == "ok"
    assert result["repo"] == "owner/flow-qa-rails"
    assert result["issues_created"] == 2

    # Verify gh repo create was called
    create_calls = [c for c in mock_run.call_args_list
                    if "repo" in str(c) and "create" in str(c)]
    assert len(create_calls) >= 1

    # Verify gh issue create was called for each issue
    issue_calls = [c for c in mock_run.call_args_list
                   if "issue" in str(c) and "create" in str(c)]
    assert len(issue_calls) == 2


def test_scaffold_writes_template_files(tmp_path):
    """scaffold() writes all template files to the clone directory."""
    clone_dir = tmp_path / "clone"

    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run:
        mock_templates.return_value = {
            "Gemfile": "gem content\n",
            "bin/ci": "#!/usr/bin/env ruby\n",
            ".qa/issues.json": json.dumps([]),
        }
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        _mod.scaffold("rails", "owner/repo", clone_dir=str(clone_dir))

    assert (clone_dir / "Gemfile").read_text() == "gem content\n"
    assert (clone_dir / "bin" / "ci").read_text() == "#!/usr/bin/env ruby\n"


def test_scaffold_gh_create_failure():
    """scaffold() returns error when gh repo create fails."""
    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run:
        mock_templates.return_value = {".qa/issues.json": "[]"}
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="already exists",
        )

        result = _mod.scaffold("rails", "owner/repo")

    assert result["status"] == "error"


def test_scaffold_git_command_failure(tmp_path):
    """scaffold() returns error when a git command fails."""
    clone_dir = tmp_path / "clone"

    call_count = 0

    def mock_run_side_effect(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        cmd = args[0] if args else kwargs.get("args", [])
        # Let gh repo create succeed, fail on git init
        if cmd[0] == "gh":
            return subprocess.CompletedProcess(
                args=cmd, returncode=0, stdout="", stderr="",
            )
        # Fail on first git command
        return subprocess.CompletedProcess(
            args=cmd, returncode=1, stdout="", stderr="git init failed",
        )

    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run", side_effect=mock_run_side_effect):
        mock_templates.return_value = {
            ".qa/issues.json": json.dumps([]),
        }
        result = _mod.scaffold("rails", "owner/repo",
                               clone_dir=str(clone_dir))

    assert result["status"] == "error"
    assert "failed" in result["message"]


def test_scaffold_default_clone_dir(tmp_path):
    """scaffold() creates a temp directory when clone_dir is not provided."""
    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run, \
         patch.object(_mod.tempfile, "mkdtemp",
                      return_value=str(tmp_path / "auto")):
        mock_templates.return_value = {
            ".qa/issues.json": json.dumps([]),
        }
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )
        result = _mod.scaffold("rails", "owner/repo")

    assert result["status"] == "ok"


def test_find_templates_default_dir():
    """find_templates() uses default templates_dir when not provided."""
    templates = _mod.find_templates("rails")
    assert "Gemfile" in templates


def test_scaffold_sets_bin_ci_executable(tmp_path):
    """scaffold() makes bin/ci executable."""
    clone_dir = tmp_path / "clone"

    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run:
        mock_templates.return_value = {
            "bin/ci": "#!/usr/bin/env ruby\n",
            ".qa/issues.json": json.dumps([]),
        }
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        _mod.scaffold("rails", "owner/repo", clone_dir=str(clone_dir))

    ci_path = clone_dir / "bin" / "ci"
    assert ci_path.stat().st_mode & 0o111  # executable bits


def test_scaffold_sets_all_bin_scripts_executable(tmp_path):
    """scaffold() makes all bin/* scripts executable, not just bin/ci."""
    clone_dir = tmp_path / "clone"

    with patch.object(_mod, "find_templates") as mock_templates, \
         patch("subprocess.run") as mock_run:
        mock_templates.return_value = {
            "bin/ci": "#!/usr/bin/env bash\n",
            "bin/test": "#!/usr/bin/env bash\n",
            "bin/build": "#!/usr/bin/env bash\n",
            ".qa/issues.json": json.dumps([]),
        }
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        _mod.scaffold("ios", "owner/repo", clone_dir=str(clone_dir))

    for script in ["ci", "test", "build"]:
        path = clone_dir / "bin" / script
        assert path.stat().st_mode & 0o111, f"bin/{script} not executable"


# --- CLI integration ---


def test_cli_missing_args(monkeypatch):
    """Missing arguments exits with error."""
    monkeypatch.setattr("sys.argv", ["scaffold-qa"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code != 0


def test_main_success():
    """main() prints JSON and exits 0 on success."""
    with patch.object(_mod, "scaffold") as mock_scaffold, \
         patch("sys.argv", ["scaffold-qa", "--framework", "rails",
                            "--repo", "owner/repo"]):
        mock_scaffold.return_value = {
            "status": "ok", "repo": "owner/repo", "issues_created": 2,
        }
        _mod.main()

    mock_scaffold.assert_called_once_with("rails", "owner/repo")


def test_main_error():
    """main() exits 1 on error."""
    with patch.object(_mod, "scaffold") as mock_scaffold, \
         patch("sys.argv", ["scaffold-qa", "--framework", "rails",
                            "--repo", "owner/repo"]), \
         pytest.raises(SystemExit) as exc_info:
        mock_scaffold.return_value = {
            "status": "error", "message": "failed",
        }
        _mod.main()

    assert exc_info.value.code == 1
