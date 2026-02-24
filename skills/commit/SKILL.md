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

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command with exit code capture:

```bash
COMMAND; EC=$?; exit $EC
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```
YYYY-MM-DDTHH:MM:SSZ [Commit] Step X — desc (exit EC)
```

Get `<branch>` from `git branch --show-current`.

---

## Process

Follow the commit process in `docs/commit-process.md` (Steps 1 through 5).

## Additional Rules

- If `bin/ci` has not been run since the last code change, warn the user before asking for approval