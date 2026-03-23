"""Post a message to Slack via curl.

Reads slack config from .flow.json in the project root. Posts to
chat.postMessage API. Supports threading via thread_ts parameter.
Fails open — if config is missing, token is invalid, or curl fails,
returns a status without raising.

Usage:
  bin/flow notify-slack --phase <phase> --message <text> [--thread-ts <ts>]
                        [--feature <name>] [--pr-url <url>]

Output (JSON to stdout):
  Success:  {"status": "ok", "ts": "1234567890.123456"}
  Skipped:  {"status": "skipped", "reason": "no slack config"}
  Error:    {"status": "error", "message": "..."}
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import PHASE_NAMES, project_root, read_flow_json


SLACK_API_URL = "https://slack.com/api/chat.postMessage"


def read_slack_config(root):
    """Read slack config from .flow.json.

    Returns dict with bot_token and channel, or None if not configured.
    """
    data = read_flow_json(root)
    if data is None:
        return None

    slack = data.get("slack")
    if not isinstance(slack, dict):
        return None

    bot_token = slack.get("bot_token")
    channel = slack.get("channel")
    if not bot_token or not channel:
        return None

    return {"bot_token": bot_token, "channel": channel}


def format_message(phase, message, feature=None, pr_url=None):
    """Format a Slack notification message.

    Includes phase name, message text, and optional feature/PR context.
    """
    phase_name = PHASE_NAMES.get(phase, phase)
    parts = [f"*{phase_name}*: {message}"]
    if feature:
        parts.append(f"Feature: {feature}")
    if pr_url:
        parts.append(f"PR: {pr_url}")
    return "\n".join(parts)


def post_message(bot_token, channel, text, thread_ts=None):
    """Post a message to Slack via curl.

    Returns dict with status and ts (message timestamp) on success.
    """
    payload = {"channel": channel, "text": text}
    if thread_ts:
        payload["thread_ts"] = thread_ts

    cmd = [
        "curl", "-s", "-X", "POST", SLACK_API_URL,
        "-H", f"Authorization: Bearer {bot_token}",
        "-H", "Content-Type: application/json; charset=utf-8",
        "-d", json.dumps(payload),
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
    except subprocess.TimeoutExpired:
        return {"status": "error", "message": "Timeout posting to Slack"}

    if result.returncode != 0:
        error = result.stderr.strip() or "curl failed"
        return {"status": "error", "message": error}

    try:
        response = json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"status": "error", "message": "Invalid JSON response from Slack"}

    if not response.get("ok"):
        error = response.get("error", "unknown error")
        return {"status": "error", "message": f"Slack API error: {error}"}

    return {"status": "ok", "ts": response.get("ts", "")}


def _parse_args(args=None):
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(description="Post to Slack")
    parser.add_argument("--phase", required=True, help="Phase name")
    parser.add_argument("--message", required=True, help="Message text")
    parser.add_argument("--thread-ts", default=None, help="Thread timestamp")
    parser.add_argument("--feature", default=None, help="Feature name")
    parser.add_argument("--pr-url", default=None, help="PR URL")
    return parser.parse_args(args)


def notify(parsed, root):
    """Core notification logic. Returns result dict."""
    config = read_slack_config(root)
    if config is None:
        return {"status": "skipped", "reason": "no slack config"}

    text = format_message(parsed.phase, parsed.message,
                          feature=parsed.feature, pr_url=parsed.pr_url)
    return post_message(config["bot_token"], config["channel"], text,
                        thread_ts=parsed.thread_ts)


def main_with_args(args, root_override=None):
    """Run the notify-slack logic with explicit args and project root.

    Used by tests to avoid subprocess overhead while testing the full flow.
    """
    parsed = _parse_args(args)
    root = root_override or project_root()
    return notify(parsed, root)


def main():
    parsed = _parse_args()
    root = project_root()
    result = notify(parsed, root)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
