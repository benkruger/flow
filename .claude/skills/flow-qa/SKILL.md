---
name: flow-qa
description: "QA the FLOW plugin locally. Clone QA repos, prime them, run a full lifecycle, and verify results — all in one command."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — runs
in the FLOW source repo against dedicated QA repos cloned to `.qa-repos/`.

## Usage

```text
/flow-qa
/flow-qa <python|rails|ios|all>
```

- `/flow-qa` — asks which framework to test (recommends `all`)
- `/flow-qa python` — runs directly against python
- `/flow-qa all` — runs against all supported frameworks sequentially

If no argument is given, use AskUserQuestion with these options
(in this order): **all**, **python**, **rails**, **ios**.

## QA Repos

| Framework | Repo | Local path |
|-----------|------|------------|
| python | `benkruger/flow-qa-python` | `.qa-repos/python` |
| rails | `benkruger/flow-qa-rails` | `.qa-repos/rails` |
| ios | `benkruger/flow-qa-ios` | `.qa-repos/ios` |

All three frameworks are supported: `python`, `rails`, and `ios`.

## Steps

Delete any stale clone, clone fresh, prime, run a full FLOW lifecycle
(Start through Complete), and verify.

### Step 1 — Fresh clone

Remove any existing clone and clone fresh:

```bash
rm -rf .qa-repos/<framework>
```

```bash
gh repo clone <owner/repo> .qa-repos/<framework>
```

### Step 2 — Prime

Run `prime-setup` with fully autonomous settings:

```bash
bin/flow prime-setup .qa-repos/<framework> --framework <framework> --skills-json '{"flow-start":{"continue":"auto"},"flow-plan":{"continue":"auto","dag":"auto"},"flow-code":{"commit":"auto","continue":"auto"},"flow-code-review":{"commit":"auto","continue":"auto","code_review_plugin":"never"},"flow-learn":{"commit":"auto","continue":"auto"},"flow-abort":"auto","flow-complete":"auto"}' --plugin-root $PWD
```

If the JSON output has `"status": "error"`, print the error and stop.

### Step 3 — Fetch an issue

Get the first open issue from the QA repo:

```bash
gh issue list --repo <owner/repo> --state open --json number --jq '.[0].number'
```

If no issues exist, print "No open issues in QA repo" and stop.

### Step 4 — Execute the flow

Write a QA continuation breadcrumb so the stop hook forces continuation
after all 6 phases complete (the branch state file is deleted by cleanup,
so this breadcrumb is the only thing that survives):

Use the Write tool to write `.flow-states/qa-pending.json` with:

```json
{"_continue_context": "QA flow complete. Return to FLOW repo at <absolute-path-to-flow-repo>, delete .flow-states/qa-pending.json via the Write tool (overwrite with empty), then run qa-verify and report results for <framework> against <owner/repo>."}
```

Change directory to the QA repo:

```bash
cd .qa-repos/<framework>
```

Invoke the FLOW lifecycle using the Skill tool:

> Invoke `/flow:flow-start --auto fix issue #<N>` where `<N>` is the
> issue number from Step 3.

The flow will run all 6 phases autonomously (Start through Complete).
After `flow-complete` finishes, the worktree is cleaned up and the
PR is merged.

### Step 5 — Return to FLOW repo

After all phases complete, the worktree has been removed and the Bash
cwd may be invalid. Change back to the FLOW repo root using the
absolute path you were in before Step 4 (not a relative `cd ../..`
which may fail if the cwd was deleted):

```bash
cd <absolute-path-to-flow-repo>
```

Delete the QA breadcrumb file by overwriting it with the Write tool
(write `{}` to `.flow-states/qa-pending.json`), then delete it:

```bash
rm .flow-states/qa-pending.json
```

### Step 6 — Verify

Run verification:

```bash
bin/flow qa-verify --framework <framework> --repo <owner/repo> --project-root .qa-repos/<framework>
```

Parse the JSON output. Report each check's pass/fail status.

### Step 7 — Report

If all checks passed, print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — PASSED (<framework>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If any check failed, print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✗ FLOW QA — FAILED (<framework>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never run QA against the FLOW repo itself — only against QA repos in `.qa-repos/`
- Report pass/fail for each check individually
- Stop on first framework failure when running `all`
- QA repos test the FLOW lifecycle, not project code quality — `bin/ci` should run tests only, no linters. If `bin/ci` fails on seed code, fix the seed (remove what doesn't belong), don't debug the linter.
- When fixing a QA repo (e.g. broken file permissions, missing files), always update the `seed` tag after pushing the fix: `git -C .qa-repos/<framework> tag -f seed` then `git -C .qa-repos/<framework> push -f origin seed`. The `qa-reset` script resets to the seed tag — if the tag points to the broken state, the fix is lost on every reset.
