"""Tests for lib/init-state.py — early state file creation for TUI visibility."""

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest
from conftest import LIB_DIR, PHASE_ORDER, make_flow_json

SCRIPT = str(LIB_DIR / "init-state.py")

# Import init-state.py for in-process unit tests of edge cases
_spec = importlib.util.spec_from_file_location("init_state", LIB_DIR / "init-state.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _current_plugin_version():
    """Read the current version from plugin.json."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())["version"]


def _run(cwd, feature_name, prompt_file=None, auto=False):
    """Run init-state.py with a feature name inside the given directory."""
    cmd = [sys.executable, SCRIPT, feature_name]
    if prompt_file is not None:
        cmd.extend(["--prompt-file", prompt_file])
    if auto:
        cmd.append("--auto")
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(cwd),
    )
    return result


# --- Happy path ---


def test_happy_path_returns_ok_json(target_project):
    """Successful run returns JSON with status, branch, state_file."""
    make_flow_json(target_project, version=_current_plugin_version(), framework="rails")
    result = _run(target_project, "test feature")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["branch"] == "test-feature"
    assert data["state_file"] == ".flow-states/test-feature.json"


# --- State file fields ---


def test_state_file_has_null_pr_fields(target_project):
    """State file created with pr_number: null and pr_url: null."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "early state test")
    state_path = target_project / ".flow-states" / "early-state-test.json"
    state = json.loads(state_path.read_text())
    assert state["pr_number"] is None
    assert state["pr_url"] is None


def test_state_file_has_null_repo(target_project):
    """State file created with repo: null."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "repo null test")
    state_path = target_project / ".flow-states" / "repo-null-test.json"
    state = json.loads(state_path.read_text())
    assert state["repo"] is None


def test_state_file_has_all_6_phases(target_project):
    """State file must have all 6 phases with correct names."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "six phases test")
    state_path = target_project / ".flow-states" / "six-phases-test.json"
    state = json.loads(state_path.read_text())
    expected_names = {
        "flow-start": "Start",
        "flow-plan": "Plan",
        "flow-code": "Code",
        "flow-code-review": "Code Review",
        "flow-learn": "Learn",
        "flow-complete": "Complete",
    }
    assert len(state["phases"]) == 6
    for key, name in expected_names.items():
        assert state["phases"][key]["name"] == name


def test_state_file_phase_1_in_progress(target_project):
    """Phase 1 should be in_progress with timestamps set."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "phase status test")
    state_path = target_project / ".flow-states" / "phase-status-test.json"
    state = json.loads(state_path.read_text())
    start_phase = state["phases"]["flow-start"]
    assert start_phase["status"] == "in_progress"
    assert start_phase["started_at"] is not None
    assert start_phase["session_started_at"] is not None
    assert start_phase["visit_count"] == 1


def test_state_file_other_phases_pending(target_project):
    """Non-start phases should be pending with null timestamps."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "pending phases test")
    state_path = target_project / ".flow-states" / "pending-phases-test.json"
    state = json.loads(state_path.read_text())
    for key in PHASE_ORDER:
        if key == "flow-start":
            continue
        phase = state["phases"][key]
        assert phase["status"] == "pending"
        assert phase["started_at"] is None
        assert phase["session_started_at"] is None
        assert phase["visit_count"] == 0


def test_state_file_has_required_top_level_fields(target_project):
    """State file has all required top-level fields."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "fields test")
    state_path = target_project / ".flow-states" / "fields-test.json"
    state = json.loads(state_path.read_text())
    assert state["schema_version"] == 1
    assert state["branch"] == "fields-test"
    assert state["current_phase"] == "flow-start"
    assert state["notes"] == []
    assert state["phase_transitions"] == []
    assert state["session_id"] is None
    assert state["transcript_path"] is None


def test_state_file_has_files_block(target_project):
    """State file must have a files block with plan, dag, log, and state."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "files block test")
    state_path = target_project / ".flow-states" / "files-block-test.json"
    state = json.loads(state_path.read_text())
    files = state["files"]
    assert files["plan"] is None
    assert files["dag"] is None
    assert files["log"] == ".flow-states/files-block-test.log"
    assert files["state"] == ".flow-states/files-block-test.json"


# --- Framework propagation ---


def test_framework_from_flow_json(target_project):
    """Framework reads from .flow.json."""
    make_flow_json(target_project, version=_current_plugin_version(), framework="python")
    _run(target_project, "python framework")
    state_path = target_project / ".flow-states" / "python-framework.json"
    state = json.loads(state_path.read_text())
    assert state["framework"] == "python"


def test_framework_defaults_to_rails(target_project):
    """Framework defaults to rails when not specified."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "default framework")
    state_path = target_project / ".flow-states" / "default-framework.json"
    state = json.loads(state_path.read_text())
    assert state["framework"] == "rails"


# --- Skills propagation ---


def test_skills_from_flow_json(target_project):
    """Skills config copied from .flow.json to state file."""
    skills = {"flow-start": {"continue": "manual"}}
    make_flow_json(target_project, version=_current_plugin_version(), skills=skills)
    _run(target_project, "skills config")
    state_path = target_project / ".flow-states" / "skills-config.json"
    state = json.loads(state_path.read_text())
    assert state["skills"] == skills


def test_skills_omitted_when_not_in_flow_json(target_project):
    """No skills key in state when .flow.json has no skills."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "no skills")
    state_path = target_project / ".flow-states" / "no-skills.json"
    state = json.loads(state_path.read_text())
    assert "skills" not in state


# --- --auto flag ---


def test_auto_flag_overrides_skills(target_project):
    """--auto flag overrides skills to fully autonomous preset."""
    manual_skills = {"flow-start": {"continue": "manual"}}
    make_flow_json(target_project, version=_current_plugin_version(), skills=manual_skills)
    _run(target_project, "auto override", auto=True)
    state_path = target_project / ".flow-states" / "auto-override.json"
    state = json.loads(state_path.read_text())
    assert state["skills"]["flow-start"]["continue"] == "auto"
    assert state["skills"]["flow-code"]["commit"] == "auto"
    assert state["skills"]["flow-code-review"]["code_review_plugin"] == "never"


def test_auto_flag_sets_skills_when_absent(target_project):
    """--auto sets skills even when .flow.json has no skills key."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "auto no skills", auto=True)
    state_path = target_project / ".flow-states" / "auto-no-skills.json"
    state = json.loads(state_path.read_text())
    assert "skills" in state
    assert state["skills"]["flow-start"]["continue"] == "auto"


# --- Prompt storage ---


def test_prompt_from_prompt_file(target_project):
    """--prompt-file reads content and stores in state."""
    make_flow_json(target_project, version=_current_plugin_version())
    prompt_path = target_project / ".flow-states" / "test-prompt-file"
    prompt_path.parent.mkdir(parents=True, exist_ok=True)
    prompt_path.write_text("fix issue #42 with special chars: && | ;")
    result = _run(target_project, "prompt file test", prompt_file=str(prompt_path))
    assert result.returncode == 0, result.stderr
    state_path = target_project / ".flow-states" / "prompt-file-test.json"
    state = json.loads(state_path.read_text())
    assert state["prompt"] == "fix issue #42 with special chars: && | ;"
    assert not prompt_path.exists()


def test_prompt_defaults_to_feature_name(target_project):
    """Without --prompt-file, prompt defaults to feature name."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "default prompt")
    state_path = target_project / ".flow-states" / "default-prompt.json"
    state = json.loads(state_path.read_text())
    assert state["prompt"] == "default prompt"


def test_prompt_file_not_found_returns_error(target_project):
    """--prompt-file with nonexistent path returns error."""
    make_flow_json(target_project, version=_current_plugin_version())
    result = _run(target_project, "error test", prompt_file="/nonexistent/prompt-file")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


# --- Frozen phases file ---


def test_frozen_phases_file_created(target_project):
    """Creates .flow-states/<branch>-phases.json from flow-phases.json."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "frozen phases")
    frozen = target_project / ".flow-states" / "frozen-phases-phases.json"
    assert frozen.exists()


def test_frozen_phases_file_matches_source(target_project):
    """Frozen phases file must match flow-phases.json content."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "phases match")
    frozen = target_project / ".flow-states" / "phases-match-phases.json"
    frozen_data = json.loads(frozen.read_text())
    source_path = Path(__file__).resolve().parent.parent / "flow-phases.json"
    source_data = json.loads(source_path.read_text())
    assert frozen_data == source_data


# --- Log file ---


def test_log_file_created(target_project):
    """Log file created with init-state entry."""
    make_flow_json(target_project, version=_current_plugin_version())
    _run(target_project, "log test")
    log_path = target_project / ".flow-states" / "log-test.log"
    assert log_path.exists()
    log = log_path.read_text()
    assert "[Phase 1]" in log


# --- Error cases ---


def test_missing_feature_name_fails(target_project, monkeypatch, capsys):
    """Running without a feature name exits with error."""
    monkeypatch.chdir(target_project)
    monkeypatch.setattr("sys.argv", ["init-state"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "feature name" in data["message"].lower()


def test_missing_flow_json_returns_error(target_project):
    """Missing .flow.json returns error JSON."""
    flow_json = target_project / ".flow.json"
    if flow_json.exists():
        flow_json.unlink()
    result = _run(target_project, "no flow json")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


# --- Branch name derivation ---


def test_branch_name_derived_from_feature(target_project):
    """Branch name derived correctly from feature words."""
    make_flow_json(target_project, version=_current_plugin_version())
    result = _run(target_project, "Invoice Pdf Export")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["branch"] == "invoice-pdf-export"


def test_branch_name_truncated_at_32(target_project):
    """Branch names exceeding 32 chars truncated at word boundary."""
    make_flow_json(target_project, version=_current_plugin_version())
    result = _run(target_project, "this is a very long feature name that exceeds limit")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert len(data["branch"]) <= 32
    assert not data["branch"].endswith("-")


def test_branch_name_single_long_word():
    """Single word >32 chars with no hyphens truncates at 32 (in-process)."""
    result = _mod._branch_name("a" * 40)
    assert len(result) == 32
    assert result == "a" * 32


# --- CLI integration ---


def test_cli_via_bin_flow(target_project, monkeypatch):
    """bin/flow init-state routes to lib/init-state.py correctly."""
    make_flow_json(target_project, version=_current_plugin_version())
    bin_flow = Path(__file__).resolve().parent.parent / "bin" / "flow"
    result = subprocess.run(
        [str(bin_flow), "init-state", "cli integration test"],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["branch"] == "cli-integration-test"


# --- Structural: single source of truth ---


def test_auto_skills_imported_not_defined():
    """init-state.py must import AUTO_SKILLS from flow_utils, not define its own."""
    source = (LIB_DIR / "init-state.py").read_text()
    # Must not have a top-level assignment like AUTO_SKILLS = {
    for line in source.splitlines():
        stripped = line.lstrip()
        if stripped.startswith("AUTO_SKILLS") and "=" in stripped and "import" not in stripped:
            pytest.fail("init-state.py defines AUTO_SKILLS locally; it should import from flow_utils")
