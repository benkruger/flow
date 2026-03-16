# Filing Issues

## The Pattern

1. Write the issue body to `.flow-issue-body` in the project
   root using the Write tool
2. Call `bin/flow issue --title "..." --body-file .flow-issue-body`
3. The script reads the file, deletes it, then creates the issue

## Rules

- Never pass body text as a command line argument — special
  characters trigger the Bash hook validator
- Never delete `.flow-issue-body` yourself — the script handles
  cleanup after reading
- Always use `bin/flow issue` — never call `gh issue create`
  directly

## Content Standards

- **Make every decision in the issue.** Never write "decide
  whether to" or "choose between." If you know the answer,
  state it. If you genuinely don't know, say what you'd need
  to find out — not "pick one of these options."
- **Include the WHY for every decision.** A future session
  has no memory of this conversation. State the reasoning,
  not just the conclusion. "Skip X" is incomplete. "Skip X
  because Y blocks it at runtime — adding it would be
  actively misleading" is complete.
- **Write for a cold start.** The issue is the only context
  a future session has. Include: what's wrong, why it
  matters, what to do, which files to touch, and how to
  verify.
- **File independent issues in parallel.** Use different
  temp file names (e.g., `.flow-issue-body-1`,
  `.flow-issue-body-2`) and launch all Write + `bin/flow
  issue` calls concurrently.

## Problem vs Solution Boundary

Issues describe problems, not solutions. The solution comes
from Plan phase decomposition after proper codebase
exploration.

**Include:**

- Observable behavior and evidence (state file values,
  error messages, screenshots)
- Why it matters — what breaks, what the user experiences
- Where to look — which files are involved, what to
  investigate
- How to reproduce — steps or conditions that trigger it

**Omit until proper research:**

- Fix sections — speculative solutions not validated by
  reading the code
- Root cause analysis — unless you traced the actual code
  path, a guess is not analysis
- Prescribed code changes — naming files to investigate
  is fine, prescribing specific changes is premature
