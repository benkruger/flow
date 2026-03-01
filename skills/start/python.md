# Start — Python Framework Instructions

## Steps 3-5: CI Baseline

Python projects have no dependency upgrade step (no Gemfile). Run CI
baseline and fix any failures before proceeding.

### Step 3 — Baseline `bin/ci`

```bash
bin/ci
```

- **Passes** — note as baseline and continue to Done
- **Fails** — launch the CI fix sub-agent (see Step 4). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — use `/flow:commit` to commit the fix, then continue to Done
  - **Not fixed** — stop and report to the user what is failing

### Step 4 — CI fix sub-agent

When `bin/ci` fails in Step 3, launch a sub-agent to diagnose
and fix the failures. Use the Agent tool:

- `subagent_type`: `"general-purpose"`
- `model`: `"sonnet"`
- `description`: `"Fix bin/ci failures"`

Provide these instructions (fill in the worktree path and bin/ci output):

> You are fixing CI failures in a Python worktree.
> Worktree: `<worktree path>`
> cd into the worktree before running any commands.
>
> The `bin/ci` output:
> <paste the full bin/ci output>
>
> Use the Glob and Read tools to explore code — do not use Bash for file checks.
>
> Fix the failures in this order:
>
> 1. **Lint violations** — read the lint output carefully. Fix the code
>    to satisfy the linter. Then run `bin/ci`.
> 2. **Test failures** — read the failing test and the code it tests.
>    Understand the root cause. Fix the code, not the test (unless the
>    test itself is wrong). Run `bin/test <file>` to verify,
>    then `bin/ci` for a full check.
> 3. **Coverage gaps** — identify uncovered lines from the coverage
>    report. Write the missing test, then `bin/ci`.
>
> Max 3 attempts. After each fix, run `bin/ci`. If green, report what
> was fixed and stop. If still failing after 3 attempts, report exactly
> what is failing and what was tried.
>
> Return:
>
> 1. Status: fixed / not_fixed
> 2. What was wrong
> 3. What was changed (files modified)

Wait for the sub-agent to return.

<HARD-GATE>
Do NOT proceed past Step 3 until bin/ci is green.
</HARD-GATE>

### Step 5 — Commit fixes (if any)

If the CI fix sub-agent made changes, use `/flow:commit` to commit them.

If baseline was already green, skip this step.

## Report additions

Include in the Done report:

- Whether baseline `bin/ci` was clean
- Confirmation `bin/ci` is green
