# Code Review Scope — Diff Boundary Test

## The Rule

If a Code Review finding is in a file that appears in
`git diff origin/main...HEAD`, it is **in-scope** — fix it during
Step 4. No exceptions.

Out-of-scope means the finding is in a file the PR did not touch
AND the PR did not cause the problem. Only then may it be filed
as an issue.

## Why

Filing a "tech debt" issue for a problem the current PR introduced
is avoidance, not triage. The work created the problem — the work
fixes the problem. Deferring it creates work-from-work: a future
session must understand the context, plan the fix, and run the
lifecycle again for something that could have been fixed in 5 minutes
during Code Review.

## How to Apply

During Code Review Step 3 (Triage), for every finding classified
as "real":

1. Check whether the file appears in the PR diff
2. If yes → in-scope, route to Step 4
3. If no → did this PR's changes make the finding true?
   Read the finding's claim, then check whether the PR's
   code changes are what caused it. If removing the PR
   would make the finding go away, it is in-scope — fix
   it regardless of which file it is in.
4. If no to both → out-of-scope, file an issue

This applies to all finding types: bugs, structural issues,
duplicate code, missing abstractions, naming problems, documentation
drift. "Low severity" and "simplicity" findings in PR-touched files
are still in-scope.

## New Rules and Pre-Existing Violations

When a PR adds a new rule (`.claude/rules/*.md`) that retroactively
flags existing code as a violation, the pre-existing violations in
files NOT touched by the PR are out-of-scope. The rule defines
expectations going forward — it does not make prior code "caused by
the PR." File an issue for the pre-existing violations.

Pre-existing violations in files the PR DID touch are in-scope per
the standard diff boundary test (Step 1 above). The file is in the
diff — fix it.

## In-Scope Means Fix, Not File

Never file a GitHub issue for an in-scope finding — not even one
you intend to close immediately. In-scope findings go directly to
Step 4 for fixing. Filing and closing an issue in the same PR adds
overhead (API calls, issue noise) without benefit. The diff boundary
test already decided the finding belongs in this PR.
