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
This is your `FLOW_SLACK_TOKEN`.

### 4. Invite the Bot to a Channel

1. Open the Slack channel where you want FLOW notifications
2. Type `/invite @FLOW Notifications` (or whatever you named the app)
3. The bot must be in the channel to post messages

### 5. Get the Channel ID

1. Right-click the channel name in Slack
2. Click **View channel details**
3. At the bottom of the details panel, copy the **Channel ID** (starts with `C`)
4. This is your `FLOW_SLACK_CHANNEL`

### 6. Set Environment Variables

Set both environment variables before running `/flow:flow-prime`:

```text
export FLOW_SLACK_TOKEN=xoxb-your-token-here
export FLOW_SLACK_CHANNEL=C0123456789
```

For per-project configuration, use direnv or per-project shell profiles.

### 7. Run Prime

```text
/flow:flow-prime
```

Prime detects the environment variables, validates the token with Slack's
`auth.test` API, and writes the configuration to `.flow.json` (gitignored).

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

Slack configuration lives in `.flow.json` (managed by `/flow:flow-prime`):

```json
{
  "slack": {
    "bot_token": "xoxb-...",
    "channel": "C0123456789"
  },
  "notify": "auto"
}
```

- `notify: "auto"` — notifications are sent at each phase milestone
- `notify: "never"` — notifications are disabled (set when env vars are absent)

## Disabling Notifications

To turn off Slack notifications:

1. Unset the environment variables
2. Run `/flow:flow-prime` (or `/flow:flow-prime --reprime`)

Prime removes the `slack` key from `.flow.json` and sets `notify` to `never`.

## Troubleshooting

**Token validation warning during prime:** The token was written to
`.flow.json` but the `auth.test` check failed. Verify the token is
correct and the workspace is accessible.

**Notifications not appearing:** Check that the bot is invited to the
channel and the channel ID is correct. FLOW fails silently (fail-open)
on notification errors — check the session log for error details.

**Multiple workspaces:** One Slack App per workspace. Engineers who
work across multiple companies set `FLOW_SLACK_TOKEN` and
`FLOW_SLACK_CHANNEL` per-project context (via direnv or shell profiles).
The channel can differ per project within the same workspace.
