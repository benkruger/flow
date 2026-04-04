#!/usr/bin/env bash
# FLOW Process — SessionStart hook
#
# Scans .flow-states/ for in-progress features.
# 0 files  → exits silently
# 1 file   → resets interrupted session timing, injects awareness context
# 2+ files → injects awareness context listing all features

set -euo pipefail

STATE_DIR=".flow-states"

# No state directory or no state files — exit silently unless FLOW-enabled
if [ ! -d "$STATE_DIR" ] && [ ! -f ".flow.json" ]; then
  exit 0
fi

if [ -d "$STATE_DIR" ] && [ -z "$(ls "$STATE_DIR"/*.json 2>/dev/null)" ] && [ ! -f ".flow.json" ]; then
  exit 0
fi

# Delegate to Rust subcommand for context building
exec "$(dirname "$0")/../bin/flow" session-context
