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

Currently only `python` is supported. Rails and ios are out of scope.

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

### Step 6 — Verify

Run verification:

```bash
bin/flow qa-verify --tier 1 --framework <framework> --repo <owner/repo> --project-root .qa-repos/<framework>
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
