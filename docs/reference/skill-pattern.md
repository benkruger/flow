---
title: Phase Skill Pattern
nav_order: 12
parent: Reference
---

# Phase Skill Pattern

Every phase skill follows the same structure. Use this as the template
when building new phase skills.

---

## Standard Structure

```
1. HARD-GATE entry check (inline Python — checks previous phase complete)
2. Announce banner
3. Update state file — set phase to in_progress, record session_started_at
4. cd into worktree from state file
5. [Phase-specific work]
6. Update state file — set phase to complete, calculate cumulative_seconds
7. Invoke flow:status  ← always, right before the transition question
8. AskUserQuestion — "Ready to begin Phase X+1?"
   - Yes → invoke next phase skill via Skill tool
   - Not yet → print paused banner
```

---

## Announce Banner

```
============================================
  FLOW — Phase N: Name — STARTING
============================================
```

## Paused Banner

```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```

## Completion Banner (shown after Yes is selected)

```
============================================
  FLOW — Phase N: Name — COMPLETE
============================================
```

---

## State File Updates

**On phase entry:**
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → this phase number

**On phase exit:**
- `cumulative_seconds` → `+= (now - session_started_at)`
- `status` → `complete`
- `completed_at` → current UTC timestamp
- `session_started_at` → `null`
- `current_phase` → next phase number

---

## HARD-GATE Template

Replace `PREV` with the previous phase number and `PREV_NAME` with its name:

```bash
python3 << 'PYCHECK'
import json, subprocess, sys
from pathlib import Path

def project_root():
    r = subprocess.run(['git', 'worktree', 'list', '--porcelain'],
                       capture_output=True, text=True)
    for line in r.stdout.split('\n'):
        if line.startswith('worktree '):
            return Path(line.split(' ', 1)[1].strip())
    return Path('.')

branch = subprocess.run(['git', 'branch', '--show-current'],
                        capture_output=True, text=True).stdout.strip()
state_file = project_root() / '.claude' / 'flow-states' / f'{branch}.json'

if not state_file.exists():
    print('BLOCKED: No FLOW feature in progress. Run /flow:start first.')
    sys.exit(1)

state = json.loads(state_file.read_text())
prev = state.get('phases', {}).get('PREV', {})
if prev.get('status') != 'complete':
    print('BLOCKED: Phase PREV: PREV_NAME must be complete first.')
    sys.exit(1)
PYCHECK
```

---

## Rules Every Phase Skill Follows

- Never skip the HARD-GATE
- Always cd into the worktree before running any commands
- Always invoke `flow:status` before the transition question
- Always use AskUserQuestion for the transition — never print "type /flow:next"
- Yes → invoke next skill via Skill tool
- Not yet → paused banner only
