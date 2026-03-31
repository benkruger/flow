---
title: /flow-note
nav_order: 13
parent: Skills
---

# /flow-note

**Phase:** Any (utility skill)

**Usage:** `/flow-note <message>` or invoked automatically

Captures a correction or learning to the state file immediately.
Fast — one line of output, no interruption to flow.

---

## Automatic Invocation

Claude invokes this automatically whenever the user:

- Corrects a mistake Claude made
- Disagrees with a response
- Says something was wrong or misunderstood

The note is captured before Claude replies.

---

## Explicit Invocation

```text
/flow-note Never assume branch-behind is unlikely in a multi-session workflow
```

---

## What Gets Stored

```json
{
  "phase": "flow-code",
  "phase_name": "Code",
  "timestamp": "2026-02-20T14:23:00Z",
  "type": "correction",
  "note": "Never assume branch-behind is unlikely — multiple active sessions means branches regularly fall behind main"
}
```

Notes are stored in `state["notes"]` — they survive compaction and
session restarts. Learn reads them as one of its primary sources.

---

## Rules

- Written as reusable patterns, not specific complaints
- Silent if no feature is in progress — never blocks a session
- Used by Learn as a guaranteed record that survives compaction
