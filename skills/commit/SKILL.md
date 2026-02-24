---
name: commit
description: "Review the full diff, approve or deny, then git add + commit + push. Use at every commit checkpoint in the FLOW workflow."
---

# FLOW Commit

Review all pending changes as a diff before committing. You must get explicit approval before touching git.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — flow:commit — STARTING
  Recommended model: Sonnet
============================================
```
````

On completion (whether approved or denied), print the same way:

````
```
============================================
  FLOW — flow:commit — COMPLETE
============================================
```
````

## Logging

Append a completion timestamp to every Bash command in the **same Bash call**:

```bash
COMMAND; EC=$?; date -u +"%Y-%m-%dT%H:%M:%SZ [Commit] Step X — desc (exit $EC)" >> .claude/flow-states/<branch>.log; exit $EC
```

Get `<branch>` from `git branch --show-current`. The gap between consecutive
entries is Claude's processing + command time.

---

## Process

Follow the commit process in `docs/commit-process.md` (Steps 1 through 5).

## Additional Rules

- If `bin/ci` has not been run since the last code change, warn the user before asking for approval