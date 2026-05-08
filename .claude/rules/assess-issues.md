# Assess Issues by Reading Code

When asked "is this still relevant?" or similar about an issue,
never grep for phrases from the issue body and treat matches as
confirmation. That is confirmation bias — searching for evidence
that supports the claim rather than investigating the current state.

## Required Steps

1. Fetch the issue to understand its claims
2. Read the full relevant sections of every referenced file
3. Compare current code against the issue's claims independently
4. Only then assess relevance

The assessment must come from reading the code, not from
confirming the issue's assertions. Grep snippets show fragments
but miss surrounding changes that may have already addressed
the problem.

## When the Issue Names No Files

If the issue names no files, search the codebase for the behavior
described, then read the implementation. The grep is to locate
code, not to confirm the issue. After locating the relevant code,
return to step 3 of Required Steps and compare current behavior
against the issue's claims independently.

## Check for Already-Shipped Work

Before assessing relevance, check `gh pr list --search "<issue
number>"` and `git log --all --grep "#<issue number>"` for
already-shipped work. A merged PR that referenced the issue
without closing it is strong evidence the work shipped — verify
by reading the cited code rather than trusting the PR title alone.
If the work shipped but the issue remained open, the issue may
describe a follow-up gap or may be ready to close.

## Existing Code Does Not Mean "Already Done"

When DAG analysis or codebase exploration reveals code that
appears to implement an issue's request, do not conclude the
issue is resolved. The issue may have been filed after seeing
that code — because the implementation is incomplete, covers
the wrong scope, or has a gap the filer identified.

Compare what the issue asks for against what the existing code
actually does. If they differ in any dimension (scope, paths,
conditions), the issue is not done — it is asking for the delta.
