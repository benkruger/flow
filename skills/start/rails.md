# Start — Rails Framework Instructions

## Steps 3-7: Dependency Upgrade and CI

### Step 3 — Baseline `bin/ci`

```bash
bin/ci
```

- **Passes** — note as baseline and continue
- **Fails** — launch the CI fix sub-agent (see Step 6). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — use `/flow:commit` to commit the fix, then continue
  - **Not fixed** — stop and report to the user what is failing

### Step 4 — Upgrade gems

```bash
bundle update --all
```

### Step 5 — Post-update `bin/ci`

```bash
bin/ci
```

- **Passes** — continue to Step 7
- **Fails** — launch the CI fix sub-agent (see Step 6). Pass the full
  `bin/ci` output. After the sub-agent returns:
  - **Fixed** — continue to Step 7 (Gemfile.lock + fixes committed together)
  - **Not fixed** — stop and report to the user what is failing

### Step 6 — CI fix sub-agent

When `bin/ci` fails in Step 3 or Step 5, launch a sub-agent to diagnose
and fix the failures. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `model`: `"sonnet"`
- `description`: `"Fix bin/ci failures"`

Provide these instructions (fill in the worktree path and bin/ci output):

> You are fixing CI failures in a Rails worktree.
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
> 1. **RuboCop violations** — ALWAYS run `rubocop -A` first. This
>    auto-corrects most violations. Then run `bin/ci`. If violations
>    remain, fix the code manually to satisfy the cop.
> 2. **Test failures** — read the failing test and the code it tests.
>    Understand the root cause. Fix the code, not the test (unless the
>    test itself is wrong). Run `bin/rails test <file>` to verify,
>    then `bin/ci` for a full check.
> 3. **Coverage gaps** — read `test/coverage/uncovered.txt` to see exactly
>    which lines are uncovered. Write the missing test, then `bin/ci`
>
> **Never modify `.rubocop.yml` or any RuboCop configuration.**
> Fix the code, never the rules. Do not add exclusions or disable cops.
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
Do NOT proceed past Step 3 or Step 5 until bin/ci is green.
</HARD-GATE>

### Step 7 — Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` + any gem fixes).

## Report additions

Include in the Done report:

- Whether baseline `bin/ci` was clean
- Which gems were upgraded (`git diff Gemfile.lock` summary)
- Confirmation `bin/ci` is green
