"""Tests for lib/update-pr-body.py — PR body artifact and section management."""

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "update-pr-body.py")

# Import update-pr-body.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "update_pr_body", LIB_DIR / "update-pr-body.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- _build_artifact_line ---


def test_build_artifact_line_returns_formatted_markdown():
    """Artifact line uses bold label and backtick-wrapped value."""
    result = _mod._build_artifact_line("Plan file", "/path/to/plan.md")
    assert result == "- **Plan file**: `/path/to/plan.md`"


# --- _ensure_artifacts_section ---


def test_ensure_artifacts_section_inserts_after_what():
    """Inserts ## Artifacts section after ## What paragraph."""
    body = "## What\n\nFeature Title."
    result = _mod._ensure_artifacts_section(body)
    assert "## Artifacts" in result
    assert result.index("## What") < result.index("## Artifacts")


def test_ensure_artifacts_section_no_what_heading():
    """Appends ## Artifacts when body has no ## What heading."""
    body = "Some other content."
    result = _mod._ensure_artifacts_section(body)
    assert "## Artifacts" in result
    assert result.startswith("Some other content.")


def test_ensure_artifacts_section_idempotent():
    """Does not duplicate section when it already exists."""
    body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Session log**: `/path`"
    result = _mod._ensure_artifacts_section(body)
    assert result.count("## Artifacts") == 1


# --- _add_artifact_to_body ---


def test_add_artifact_to_body_adds_new_line():
    """Adds a new artifact line to an existing ## Artifacts section."""
    body = "## What\n\nFeature Title.\n\n## Artifacts\n"
    result = _mod._add_artifact_to_body(body, "Plan file", "/plans/x.md")
    assert "- **Plan file**: `/plans/x.md`" in result


def test_add_artifact_to_body_replaces_existing_same_label():
    """Replaces an existing artifact line with the same label."""
    body = (
        "## What\n\nFeature Title.\n\n## Artifacts\n\n"
        "- **Plan file**: `/old/path.md`"
    )
    result = _mod._add_artifact_to_body(body, "Plan file", "/new/path.md")
    assert "- **Plan file**: `/new/path.md`" in result
    assert "/old/path.md" not in result
    assert result.count("Plan file") == 1


def test_add_artifact_to_body_creates_section_if_missing():
    """Creates ## Artifacts section when body has none."""
    body = "## What\n\nFeature Title."
    result = _mod._add_artifact_to_body(body, "Session log", "/path/log.jsonl")
    assert "## Artifacts" in result
    assert "- **Session log**: `/path/log.jsonl`" in result


# --- _add_artifact_to_body: multiple pairs ---


def test_add_artifact_multiple_pairs():
    """Calling _add_artifact_to_body twice adds both artifacts to the body."""
    body = "## What\n\nFeature Title.\n\n## Artifacts\n"
    body = _mod._add_artifact_to_body(body, "Plan file", "/plans/x.md")
    body = _mod._add_artifact_to_body(body, "Session log", "/logs/y.jsonl")
    assert "- **Plan file**: `/plans/x.md`" in body
    assert "- **Session log**: `/logs/y.jsonl`" in body


# --- _build_details_block ---


def test_build_details_block_returns_collapsible_html():
    """Returns a details block with heading, summary, and fenced code."""
    result = _mod._build_details_block(
        "State File", ".flow-states/b.json", '{"key": "value"}', "json"
    )
    assert "## State File" in result
    assert "<details>" in result
    assert "<summary>.flow-states/b.json</summary>" in result
    assert "```json" in result
    assert '{"key": "value"}' in result
    assert "</details>" in result


def test_build_details_block_text_format():
    """Uses text format for non-json content."""
    result = _mod._build_details_block(
        "Session Log", ".flow-states/b.log", "line 1\nline 2", "text"
    )
    assert "```text" in result
    assert "line 1\nline 2" in result


# --- _fence_for_content ---


def test_fence_for_content_no_backticks():
    """Content with no backticks returns triple-backtick fence."""
    result = _mod._fence_for_content("plain text without any fences")
    assert result == "```"


def test_fence_for_content_triple_backticks():
    """Content with triple backticks returns 4-backtick fence."""
    result = _mod._fence_for_content("before\n```python\ncode\n```\nafter")
    assert result == "````"


def test_fence_for_content_quad_backticks():
    """Content with 4 backticks returns 5-backtick fence."""
    result = _mod._fence_for_content("before\n````text\ncontent\n````\nafter")
    assert result == "`````"


def test_fence_for_content_mixed_lengths():
    """Content with mixed backtick runs uses the longest."""
    result = _mod._fence_for_content("```python\ncode\n```\n\n````xml\ndata\n````")
    assert result == "`````"


# --- _build_details_block with nested fences ---


def test_build_details_block_nested_fences():
    """Content with inner fences uses longer outer fence."""
    content = "# DAG\n\n```xml\n<dag/>\n```\n\n```python\nprint('hi')\n```"
    result = _mod._build_details_block("DAG Analysis", "dag.md", content, "text")
    lines = result.split("\n")
    # Outer fence must be longer than 3 backticks
    fence_lines = [line for line in lines if line.startswith("````")]
    assert len(fence_lines) >= 2, "Expected at least open and close 4+ backtick fences"
    # Inner triple-backtick fences appear verbatim
    assert "```xml" in result
    assert "```python" in result
    # Block structure is valid
    assert result.startswith("## DAG Analysis")
    assert result.endswith("</details>")


# --- _append_section_to_body ---


def test_append_section_to_body_appends():
    """Appends a details section to the end of the body."""
    body = "## What\n\nFeature Title."
    result = _mod._append_section_to_body(
        body, "State File", ".flow-states/b.json", '{"k": "v"}', "json"
    )
    assert body in result
    assert "## State File" in result
    assert "<details>" in result


def test_append_section_replaces_if_heading_exists():
    """Replaces an existing section with the same heading (idempotent)."""
    body = (
        "## What\n\nFeature Title.\n\n"
        "## State File\n\n<details>\n<summary>old</summary>\n\n"
        "```json\nold content\n```\n\n</details>"
    )
    result = _mod._append_section_to_body(
        body, "State File", "new-summary", "new content", "json"
    )
    assert "old content" not in result
    assert "new content" in result
    assert result.count("## State File") == 1


# --- CLI end-to-end: --add-artifact ---


def test_cli_add_artifact_end_to_end(tmp_path):
    """CLI --add-artifact reads current body via gh, updates it."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'if [[ "$1" == "pr" && "$2" == "view" ]]; then\n'
        '    echo "## What"\n'
        '    echo ""\n'
        '    echo "Feature Title."\n'
        'elif [[ "$1" == "pr" && "$2" == "edit" ]]; then\n'
        '    echo "ok"\n'
        'fi\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--add-artifact",
         "--label", "Plan file",
         "--value", "/plans/x.md"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["action"] == "add_artifact"


def test_cli_add_multiple_artifacts_end_to_end(tmp_path):
    """CLI --add-artifact with repeated --label/--value adds both artifacts."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    # Capture the body passed to `gh pr edit` so we can verify both artifacts
    body_file = tmp_path / "captured_body.txt"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'if [[ "$1" == "pr" && "$2" == "view" ]]; then\n'
        '    echo "## What"\n'
        '    echo ""\n'
        '    echo "Feature Title."\n'
        'elif [[ "$1" == "pr" && "$2" == "edit" ]]; then\n'
        f'    echo "$5" > "{body_file}"\n'
        '    echo "ok"\n'
        'fi\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--add-artifact",
         "--label", "Plan file",
         "--value", "/plans/x.md",
         "--label", "Session log",
         "--value", "/logs/y.jsonl"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["action"] == "add_artifact"

    captured = body_file.read_text()
    assert "Plan file" in captured
    assert "Session log" in captured


def test_cli_add_artifact_mismatched_label_value_count(tmp_path):
    """CLI returns error when --label count does not match --value count."""
    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--add-artifact",
         "--label", "Plan file",
         "--value", "/plans/x.md",
         "--label", "Session log"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Mismatched" in data["message"]


# --- CLI end-to-end: --append-section ---


def test_cli_append_section_end_to_end(tmp_path):
    """CLI --append-section reads content from file, appends details block."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'if [[ "$1" == "pr" && "$2" == "view" ]]; then\n'
        '    echo "## What"\n'
        '    echo ""\n'
        '    echo "Feature Title."\n'
        'elif [[ "$1" == "pr" && "$2" == "edit" ]]; then\n'
        '    echo "ok"\n'
        'fi\n'
    )
    gh_stub.chmod(0o755)

    content_file = tmp_path / "state.json"
    content_file.write_text('{"key": "value"}')

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--append-section",
         "--heading", "State File",
         "--summary", ".flow-states/b.json",
         "--content-file", str(content_file),
         "--format", "json"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["action"] == "append_section"


# --- CLI error handling ---


def test_cli_missing_pr_number():
    """CLI exits with error when --pr is missing."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--add-artifact",
         "--label", "X", "--value", "Y"],
        capture_output=True, text=True,
    )
    assert result.returncode != 0


def test_cli_missing_mode_flag():
    """CLI exits with error when neither --add-artifact nor --append-section."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--pr", "42"],
        capture_output=True, text=True,
    )
    assert result.returncode != 0


def test_cli_gh_failure_returns_error(tmp_path):
    """CLI returns error JSON when gh command fails."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'echo "gh error" >&2\n'
        'exit 1\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--add-artifact",
         "--label", "Plan file",
         "--value", "/plans/x.md"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "message" in data


def test_cli_gh_edit_failure_returns_error(tmp_path):
    """CLI returns error JSON when gh pr edit fails."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'if [[ "$1" == "pr" && "$2" == "view" ]]; then\n'
        '    echo "## What"\n'
        '    echo ""\n'
        '    echo "Feature Title."\n'
        'elif [[ "$1" == "pr" && "$2" == "edit" ]]; then\n'
        '    echo "edit failed" >&2\n'
        '    exit 1\n'
        'fi\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--add-artifact",
         "--label", "Plan file",
         "--value", "/plans/x.md"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "edit failed" in data["message"]


def test_cli_append_section_missing_content_file_arg(tmp_path):
    """CLI returns error when --content-file is not provided."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'echo "## What"\n'
        'echo ""\n'
        'echo "Feature."\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--append-section",
         "--heading", "State File",
         "--summary", "s",
         "--format", "json"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Missing" in data["message"]


def test_cli_append_section_missing_content_file(tmp_path):
    """CLI returns error when --content-file does not exist."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'echo "## What"\n'
        'echo ""\n'
        'echo "Feature."\n'
    )
    gh_stub.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--append-section",
         "--heading", "State File",
         "--summary", "s",
         "--content-file", str(tmp_path / "nonexistent"),
         "--format", "json"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"


# --- _build_plain_section ---


def test_build_plain_section_returns_heading_and_content():
    """Plain section has heading, content, and end sentinel comment."""
    result = _mod._build_plain_section("Phase Timings", "| Phase | Duration |")
    assert "## Phase Timings" in result
    assert "| Phase | Duration |" in result
    assert "<!-- end:Phase Timings -->" in result
    assert "<details>" not in result


# --- _append_plain_section_to_body ---


def test_append_plain_section_appends_to_body():
    """Appends a plain section to the end of the body."""
    body = "## What\n\nFeature Title."
    result = _mod._append_plain_section_to_body(
        body, "Phase Timings", "| Phase | Duration |"
    )
    assert body in result
    assert "## Phase Timings" in result
    assert "<!-- end:Phase Timings -->" in result


def test_append_plain_section_replaces_existing():
    """Replaces an existing plain section with the same heading."""
    body = (
        "## What\n\nFeature Title.\n\n"
        "## Phase Timings\n\nold content\n\n<!-- end:Phase Timings -->"
    )
    result = _mod._append_plain_section_to_body(
        body, "Phase Timings", "new content"
    )
    assert "old content" not in result
    assert "new content" in result
    assert result.count("## Phase Timings") == 1


def test_append_plain_section_idempotent():
    """Calling twice with same content produces same result."""
    body = "## What\n\nFeature Title."
    first = _mod._append_plain_section_to_body(
        body, "Phase Timings", "| Phase | Duration |"
    )
    second = _mod._append_plain_section_to_body(
        first, "Phase Timings", "| Phase | Duration |"
    )
    assert first == second
    assert second.count("## Phase Timings") == 1


# --- CLI end-to-end: --no-collapse ---


def test_cli_no_collapse_end_to_end(tmp_path):
    """CLI --append-section --no-collapse renders plain markdown, not details."""
    stub_dir = tmp_path / "bin"
    stub_dir.mkdir()
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'if [[ "$1" == "pr" && "$2" == "view" ]]; then\n'
        '    echo "## What"\n'
        '    echo ""\n'
        '    echo "Feature Title."\n'
        'elif [[ "$1" == "pr" && "$2" == "edit" ]]; then\n'
        '    echo "ok"\n'
        'fi\n'
    )
    gh_stub.chmod(0o755)

    content_file = tmp_path / "timings.md"
    content_file.write_text("| Phase | Duration |\n|-------|----------|")

    env = os.environ.copy()
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--pr", "42",
         "--append-section",
         "--heading", "Phase Timings",
         "--content-file", str(content_file),
         "--no-collapse"],
        capture_output=True, text=True, env=env,
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["action"] == "append_section"
