---
name: cleanup
description: "Phase 10: Cleanup — remove the worktree and delete the state file. Final phase. Requires Phase 9: Reflect to be complete."
---

# SDLC Cleanup — Phase 10: Cleanup

<HARD-GATE>
Run this phase entry check as your very first action. If it exits
non-zero, stop immediately and show the error to the user.

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
state_file = project_root() / '.claude' / 'sdlc-states' / f'{branch}.json'

if not state_file.exists():
    print('BLOCKED: No SDLC feature in progress. Run /sdlc:start first.')
    sys.exit(1)

state = json.loads(state_file.read_text())
prev = state.get('phases', {}).get('9', {})
if prev.get('status') != 'complete':
    print('BLOCKED: Phase 9: Reflect must be complete before Cleanup.')
    print('Run /sdlc:reflect first.')
    sys.exit(1)
PYCHECK
```
</HARD-GATE>

## Announce

Print:

```
============================================
  SDLC — Phase 10: Cleanup — STARTING
============================================
```

## Steps

### Step 1 — Read state

Read `.claude/sdlc-states/<branch>.json` from the project root.
Note the `worktree` and `feature` values — you will need them.

### Step 2 — Confirm with user

This phase is destructive and irreversible. Use AskUserQuestion:

> "Ready to clean up feature '<feature>'?
> This will remove the worktree and delete the state file permanently."
> - **Yes, clean up** — proceed
> - **No, not yet** — stop here

### Step 3 — Navigate to project root

```bash
cd <project_root>
```

Use `git worktree list --porcelain` to find the project root if needed.
All cleanup commands must run from the project root, not from inside
the worktree.

### Step 4 — Remove the worktree

```bash
git worktree remove .worktrees/<feature-name> --force
```

This deletes the worktree directory and all its contents.

### Step 5 — Delete the state file

Delete `.claude/sdlc-states/<branch>.json`.

This is what clears the feature from the SessionStart hook. The next
session will start clean.

### Done — Mark all phases complete and print banner

Print:

```
============================================
  SDLC — Phase 10: Cleanup — COMPLETE
  Feature '<feature>' is fully done.
============================================
```

## Rules

- Never run from inside the worktree — always navigate to project root first
- Always confirm with the user before Step 4 — removal is irreversible
- State file deletion is what resets the session hook — do not skip it
