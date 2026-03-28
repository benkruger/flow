# Filing Issues

## The Pattern

1. Write the issue body to `.flow-issue-body` in the project
   root using the Write tool
2. Call `bin/flow issue --title "..." --body-file .flow-issue-body`
3. The script reads the file, deletes it, then creates the issue

## Editing Existing Issues

Use the same `.flow-issue-body` temp file pattern:

1. Write the updated body to `.flow-issue-body` using the Write tool
2. Call `gh issue edit <number> --repo <owner/repo> --body-file .flow-issue-body`
3. Delete `.flow-issue-body` yourself — `gh issue edit` does not
   auto-delete

Never write temp files to `/tmp/` — the project's `defaultMode:
"plan"` has no allow-list pattern for `/tmp/` paths, triggering
permission prompts.

## Rules

- Never pass body text as a command line argument — special
  characters trigger the Bash hook validator
- Never delete `.flow-issue-body` yourself when creating — the
  script handles cleanup after reading
- Always use `bin/flow issue` for creating — never call
  `gh issue create` directly

## Content Standards

Issues are bug reports, not design documents. Capture
the problem with zero solutioning. Research, diagnosis,
and design happen in the Plan phase after proper codebase
exploration.

- **Write for a cold start.** A future session has no
  memory of this conversation. The issue is its only
  context for the problem.
- **Describe what is broken and why it matters.** Include
  observable behavior, evidence (state file values, error
  messages, logs), and user impact.
- **Include reproduction steps.** Steps or conditions that
  trigger the problem.
- **Name files to investigate, not files to change.** Point
  to where the behavior originates so the Plan phase knows
  where to start reading.
- **File independent issues in parallel.** Use different
  temp file names (e.g., `.flow-issue-body-1`,
  `.flow-issue-body-2`) and launch all Write + `bin/flow
  issue` calls concurrently.

## Repo Routing

Most issue-filing paths target the current project (omit `--repo`):
Tech Debt, Flaky Test, Documentation Drift, and decomposed work items
all describe problems in the user's code.

FLOW process bugs — problems with the plugin itself — must target
`benkruger/flow`. Pass `--repo benkruger/flow --label "Flow"` when
filing against the plugin repo. Two skills support this:

- `flow-learn` (Phase 5) — files process gap issues with `--repo`
- `flow-create-issue` — asks the user which repo before filing

When in doubt, ask the user. Filing against the wrong repo is
worse than one extra question.

## Dependencies

When filing an issue that depends on another issue, include
a `Blocked by #N` line in the issue body. This ensures
`flow-issues` can detect the dependency via text parsing.

- `bin/flow link-blocked-by` adds these references
  automatically for decomposed issues
- Manually filed issues need the reference added by the
  author

## Never Include

- Root cause analysis — a guess is not analysis
- Proposed solutions or "open questions" about tradeoffs
- Prescribed code changes or architectural suggestions
- Diagnosis of why the bug happens — only what happens
