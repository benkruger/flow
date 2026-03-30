---
title: Interactive TUI
nav_order: 14
parent: Reference
---

# Interactive TUI

A standalone curses-based terminal application for viewing and managing
active FLOW features. Runs directly in your terminal without a Claude
session.

## Usage

```text
flow tui
```

## Views

### List View (default)

Shows all active flows with a detail panel for the selected flow:

- **Flow list** — feature name, current phase number and name, elapsed
  time, PR number
- **Detail panel** — branch, worktree path, full phase timeline with
  per-phase cumulative time, code task progress when in Code phase,
  diff stats when available, notes count, issues filed count

### Log View

Shows the last entries from the selected flow's session log, parsed
from `.flow-states/<branch>.log`.

## Keyboard Actions

| Key | Action |
|-----|--------|
| Up/Down | Navigate flow list |
| Enter | Open worktree in terminal (activates existing iTerm2 tab or opens new tab) |
| p | Open PR in browser |
| l | Show log view |
| a | Abort flow (with Y/N confirmation) |
| r | Force refresh |
| Esc | Return from log view to list view |
| q | Quit |

## Data Sources

All data is local — no network calls:

| Data | Source |
|------|--------|
| Active flows | `.flow-states/*.json` |
| Phase timeline | `state["phases"]` dict |
| Elapsed time | `state["started_at"]` |
| Code progress | `state["code_task"]`, `state["diff_stats"]` |
| Notes/issues | `state["notes"]`, `state["issues_filed"]` |
| Log entries | `.flow-states/<branch>.log` |
| PR info | `state["pr_url"]`, `state["pr_number"]` |

## Auto-Refresh

The TUI re-reads state files every 2 seconds, so changes from active
Claude sessions appear automatically.

## Abort

The abort action (`a` key) requires Y/N confirmation, then calls
`bin/flow cleanup` to close the PR, delete the remote branch, remove
the worktree, and delete the state file.

## Platform Support

macOS and Linux only. The `curses` module is part of the Python standard
library on these platforms. Windows requires `windows-curses` (not
supported).
