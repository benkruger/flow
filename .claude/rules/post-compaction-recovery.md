# Post-Compaction Recovery

FLOW writes a `compact_summary` field into
`.flow-states/<branch>/state.json` on every conversation compaction
(`compact_count` tracks how many compactions have occurred). It holds
the full pre-compaction analysis — in-flight decisions, agent
findings, classifications, rationale — beyond the brief summary the
continuation prompt surfaces.

## The Rule

On post-compaction resume during an active flow:

1. The state file's `compact_summary` field is the authoritative
   context-recovery source. Before concluding any pre-compaction
   detail is "lost," Read `.flow-states/<branch>/state.json` and
   consult `compact_summary`.
2. Never read the raw transcript JSONL
   (`~/.claude/projects/.../<session_id>.jsonl`) to recover context.
   It sits outside the project root — reads trigger permission
   prompts — and `compact_summary` already holds the recoverable
   detail. The transcript root is also locked down by
   `validate-claude-paths` Layer 3 — Edit, Write, AND Read tool
   calls all block on the transcript root regardless of flow
   state, so the hook layer mechanically refuses a Read attempt
   that would otherwise surface a permission prompt mid-flow.
3. "Lost to compaction" is not a valid conclusion until the state
   file has been read and `compact_summary` confirmed not to hold
   the detail.

## Why

The continuation prompt surfaces a brief summary. When a resumed
session needs detail beyond it — a specific finding's description, a
mid-triage classification, a decision rationale — the recovery
source is the state file, not the transcript and not a "treat it as
lost" fallback. Reaching for the transcript is both unnecessary (the
data is in `compact_summary`) and disruptive (a permission prompt on
an out-of-project path).

## How to Apply

When a resumed session is missing a pre-compaction detail, Read
`.flow-states/<branch>/state.json` and check `compact_summary` before
taking any other recovery action. Treat that field — not the raw
transcript — as the recovery surface.
