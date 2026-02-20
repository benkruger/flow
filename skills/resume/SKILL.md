---
name: resume
description: "Resume the current SDLC feature. Mid-session: re-asks the last phase transition question. New session: reads state file, shows status, then asks."
---

# SDLC Resume

This skill behaves differently depending on whether you are mid-session
or starting fresh. Choose the right path below.

---

## Path A — Mid-session (you already have context)

If you are in an active session and already know the current phase and
feature — simply re-ask the phase transition question that was most
recently declined:

Use AskUserQuestion:

> "Ready to continue Phase X: Name?"
> - **Yes, continue** — invoke the phase skill using the Skill tool
> - **Not yet** — print the paused banner and stop

The Skill to invoke maps directly to the current phase:

| Current phase | Skill to invoke |
|--------------|----------------|
| 1 — Start | `sdlc:start` |
| 2 — Research | `sdlc:research` |
| 3 — Design | `sdlc:design` |
| 4 — Plan | `sdlc:plan` |
| 5 — Implement | `sdlc:implement` |
| 6 — Test | `sdlc:test` |
| 7 — Review | `sdlc:review` |
| 8 — Ship | `sdlc:ship` |
| 9 — Reflect | `sdlc:reflect` |
| 10 — Cleanup | `sdlc:cleanup` |

---

## Path B — New session (no current context)

If this is a new session or you have no context about the current
feature, rebuild from the state file:

### Step 1 — Find the state file

```bash
python3 << 'PYCHECK'
import subprocess, sys
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
    print(f'No SDLC feature in progress on branch "{branch}".')
    sys.exit(1)

print(str(state_file))
PYCHECK
```

If no state file is found — report it and stop.

### Step 2 — cd into the worktree

Read `worktree` from the state file and cd there.

### Step 3 — Show status panel

Invoke the `sdlc:status` skill to display current state.

### Step 4 — Ask the transition question

Use AskUserQuestion:

> "Ready to continue Phase X: Name?"
> - **Yes, continue** — invoke the phase skill using the Skill tool
> - **Not yet** — print the paused banner and stop

---

## Paused Banner

When the user selects "Not yet", always print:

```
============================================
  SDLC — Paused
  Run /sdlc:resume when ready to continue.
============================================
```
