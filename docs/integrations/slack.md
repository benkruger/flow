---
title: Slack Integration
nav_order: 20
parent: Integrations
---

# Slack Integration

FLOW can post thread-per-feature notifications to a Slack channel, giving your
team passive awareness of feature progress from start to merge.

## Setup

### 1. Create a Slack App

1. Go to [api.slack.com/apps](https://api.slack.com/apps) and click **Create New App**
2. Choose **From scratch**
3. Name the app (e.g., "FLOW Notifications") and select your workspace
4. Click **Create App**

### 2. Add Bot Token Scope

1. In the app settings, go to **OAuth & Permissions**
2. Under **Bot Token Scopes**, click **Add an OAuth Scope**
3. Add `chat:write`
4. Click **Install to Workspace** and approve the permissions

### 3. Copy the Bot Token

After installing, copy the **Bot User OAuth Token** (starts with `xoxb-`).
You will enter this when enabling the FLOW plugin.

### 4. Invite the Bot to a Channel

1. Open the Slack channel where you want FLOW notifications
2. Type `/invite @FLOW Notifications` (or whatever you named the app)
3. The bot must be in the channel to post messages

### 5. Get the Channel ID

1. Right-click the channel name in Slack
2. Click **View channel details**
3. At the bottom of the details panel, copy the **Channel ID** (starts with `C`)

### 6. Enable the FLOW Plugin

When you enable the FLOW plugin in Claude Code, you will be prompted for:

- **Slack bot token** — the `xoxb-` token from Step 3 (stored in your system keychain)
- **Slack channel ID** — the `C...` channel ID from Step 5

Both fields are optional. Skip them to disable Slack notifications.

## How It Works

Each FLOW feature gets **one Slack thread** in the configured channel.
The thread is the complete narrative of the feature from start to merge:

| Phase | Thread Message |
|-------|---------------|
| Start | Initial message (creates thread): feature name, PR link |
| Plan | Reply: task count, plan summary |
| Code | Reply: phase complete |
| Code Review | Reply: review findings summary |
| Learn | Reply: rules filed, changes made |
| Complete | Reply: merged, end-to-end timeline |

## Configuration

Slack credentials are managed by Claude Code's plugin `userConfig` system.
The bot token is stored in your system keychain (macOS) or protected
credentials file (other platforms) — never in plaintext on disk.

The plugin declares two config fields in `plugin.json`:

- `slack_bot_token` — sensitive, keychain-backed
- `slack_channel` — not sensitive

At runtime, these are available as environment variables
`CLAUDE_PLUGIN_CONFIG_slack_bot_token` and `CLAUDE_PLUGIN_CONFIG_slack_channel`.

## Disabling Notifications

To turn off Slack notifications, reconfigure the plugin and clear the
bot token and channel values.

## Troubleshooting

**Notifications not appearing:** Check that the bot is invited to the
channel and the channel ID is correct. FLOW fails silently (fail-open)
on notification errors — check the session log for error details.

**Multiple workspaces:** One Slack App per workspace. The plugin
userConfig is per Claude Code installation. Engineers who work across
multiple workspaces reconfigure the plugin when switching contexts.
