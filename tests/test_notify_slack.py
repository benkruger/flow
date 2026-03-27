"""Tests for lib/notify-slack.py — posts messages to Slack via curl."""

import json
import subprocess
from unittest.mock import patch

from conftest import import_lib, make_flow_json

# --- read_slack_config ---


def test_read_config_with_both_values(tmp_path):
    """Returns bot_token and channel when both are present in .flow.json."""
    mod = import_lib("notify-slack.py")
    make_flow_json(tmp_path, bot_token="xoxb-test-token", channel="C12345")
    config = mod.read_slack_config(tmp_path)
    assert config["bot_token"] == "xoxb-test-token"
    assert config["channel"] == "C12345"


def test_read_config_missing_slack_key(tmp_path):
    """Returns None when .flow.json has no slack key."""
    mod = import_lib("notify-slack.py")
    make_flow_json(tmp_path)  # No bot_token/channel → no slack block
    config = mod.read_slack_config(tmp_path)
    assert config is None


def test_read_config_missing_file(tmp_path):
    """Returns None when .flow.json does not exist."""
    mod = import_lib("notify-slack.py")
    config = mod.read_slack_config(tmp_path)
    assert config is None


def test_read_config_corrupt_json(tmp_path):
    """Returns None when .flow.json is corrupt."""
    mod = import_lib("notify-slack.py")
    (tmp_path / ".flow.json").write_text("{bad json")
    config = mod.read_slack_config(tmp_path)
    assert config is None


def test_read_config_missing_bot_token(tmp_path):
    """Returns None when slack block has channel but no bot_token."""
    mod = import_lib("notify-slack.py")
    data = {"flow_version": "0.36.2", "framework": "rails", "slack": {"channel": "C12345"}}
    (tmp_path / ".flow.json").write_text(json.dumps(data))
    config = mod.read_slack_config(tmp_path)
    assert config is None


def test_read_config_missing_channel(tmp_path):
    """Returns None when slack block has bot_token but no channel."""
    mod = import_lib("notify-slack.py")
    data = {"flow_version": "0.36.2", "framework": "rails", "slack": {"bot_token": "xoxb-test"}}
    (tmp_path / ".flow.json").write_text(json.dumps(data))
    config = mod.read_slack_config(tmp_path)
    assert config is None


# --- format_message ---


def test_format_message_basic():
    """Formats a basic phase message."""
    mod = import_lib("notify-slack.py")
    result = mod.format_message("flow-start", "Feature started")
    assert "Start" in result
    assert "Feature started" in result


def test_format_message_with_feature_and_pr():
    """Includes feature name and PR URL when provided."""
    mod = import_lib("notify-slack.py")
    result = mod.format_message(
        "flow-start",
        "Feature started",
        feature="Invoice Export",
        pr_url="https://github.com/org/repo/pull/42",
    )
    assert "Invoice Export" in result
    assert "https://github.com/org/repo/pull/42" in result


def test_format_message_unknown_phase():
    """Handles unknown phase gracefully."""
    mod = import_lib("notify-slack.py")
    result = mod.format_message("unknown-phase", "Some message")
    assert "Some message" in result


# --- post_message ---


def test_post_message_success(tmp_path):
    """Returns ts from successful Slack API response."""
    mod = import_lib("notify-slack.py")
    slack_response = {"ok": True, "ts": "1234567890.123456"}
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        result = mod.post_message("xoxb-token", "C12345", "Hello")
    assert result["status"] == "ok"
    assert result["ts"] == "1234567890.123456"


def test_post_message_with_thread_ts(tmp_path):
    """Passes thread_ts to curl for threaded replies."""
    mod = import_lib("notify-slack.py")
    slack_response = {"ok": True, "ts": "1234567890.654321"}
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        result = mod.post_message("xoxb-token", "C12345", "Reply", thread_ts="1234567890.123456")
    assert result["status"] == "ok"
    call_args = mock_run.call_args[0][0]
    call_str = " ".join(call_args)
    assert "thread_ts" in call_str


def test_post_message_slack_error():
    """Returns error when Slack API returns ok=false."""
    mod = import_lib("notify-slack.py")
    slack_response = {"ok": False, "error": "channel_not_found"}
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        result = mod.post_message("xoxb-token", "C12345", "Hello")
    assert result["status"] == "error"
    assert "channel_not_found" in result["message"]


def test_post_message_curl_failure():
    """Returns error when curl returns non-zero exit code."""
    mod = import_lib("notify-slack.py")
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Connection refused",
        )
        result = mod.post_message("xoxb-token", "C12345", "Hello")
    assert result["status"] == "error"


def test_post_message_curl_timeout():
    """Returns error when curl times out."""
    mod = import_lib("notify-slack.py")
    with patch("subprocess.run") as mock_run:
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="curl", timeout=15)
        result = mod.post_message("xoxb-token", "C12345", "Hello")
    assert result["status"] == "error"
    assert "timeout" in result["message"].lower()


def test_post_message_invalid_json_response():
    """Returns error when Slack returns non-JSON response."""
    mod = import_lib("notify-slack.py")
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="<html>error</html>",
            stderr="",
        )
        result = mod.post_message("xoxb-token", "C12345", "Hello")
    assert result["status"] == "error"


# --- CLI integration ---


def test_cli_no_config_returns_skipped(tmp_path, monkeypatch, capsys):
    """CLI returns skipped when no .flow.json exists."""
    mod = import_lib("notify-slack.py")
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["notify-slack.py", "--phase", "flow-start", "--message", "test"])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "skipped"


def test_cli_with_config_posts_message(tmp_path):
    """CLI posts message when config exists (mocked curl)."""
    make_flow_json(tmp_path, bot_token="xoxb-test", channel="C12345")
    slack_response = {"ok": True, "ts": "1234567890.123456"}

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        mod = import_lib("notify-slack.py")
        result = mod.main_with_args(
            ["--phase", "flow-start", "--message", "test"],
            root_override=tmp_path,
        )
    assert result["status"] == "ok"
    assert result["ts"] == "1234567890.123456"


def test_cli_with_thread_ts(tmp_path):
    """CLI passes thread_ts for replies."""
    make_flow_json(tmp_path, bot_token="xoxb-test", channel="C12345")
    slack_response = {"ok": True, "ts": "1234567890.654321"}

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        mod = import_lib("notify-slack.py")
        result = mod.main_with_args(
            ["--phase", "flow-plan", "--message", "Plan complete", "--thread-ts", "1234567890.123456"],
            root_override=tmp_path,
        )
    assert result["status"] == "ok"


def test_cli_returns_valid_json(tmp_path, monkeypatch, capsys):
    """CLI produces valid JSON on stdout."""
    mod = import_lib("notify-slack.py")
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["notify-slack.py", "--phase", "flow-start", "--message", "test"])

    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert "status" in data


def test_main_with_args_no_config_returns_skipped(tmp_path):
    """main_with_args returns skipped when no config exists."""
    mod = import_lib("notify-slack.py")
    result = mod.main_with_args(
        ["--phase", "flow-start", "--message", "test"],
        root_override=tmp_path,
    )
    assert result["status"] == "skipped"


def test_notify_function_directly(tmp_path):
    """notify() returns ok with mocked curl when config exists."""
    mod = import_lib("notify-slack.py")
    make_flow_json(tmp_path, bot_token="xoxb-test", channel="C12345")
    parsed = mod._parse_args(["--phase", "flow-start", "--message", "test"])
    slack_response = {"ok": True, "ts": "1234567890.999999"}
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(slack_response),
            stderr="",
        )
        result = mod.notify(parsed, tmp_path)
    assert result["status"] == "ok"
    assert result["ts"] == "1234567890.999999"
