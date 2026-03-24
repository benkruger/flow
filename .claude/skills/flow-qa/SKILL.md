---
name: flow-qa
description: "QA the FLOW plugin locally. Clone QA repos, prime them, run tiered QA, and verify results — all in-session."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — runs
in the FLOW source repo against dedicated QA repos cloned to `.qa-repos/`.

## Usage

```text
/flow-qa
/flow-qa --start
/flow-qa --stop
/flow-qa --run <framework|all>
/flow-qa --reset <framework|all>
/flow-qa --tier <1|2|3> --framework <name>
```

- `/flow-qa` — show setup status (which QA repos are cloned and primed)
- `/flow-qa --start` — clone and prime QA repo(s) into `.qa-repos/`
- `/flow-qa --stop` — remove `.qa-repos/` directory
- `/flow-qa --run <framework|all>` — run Tier 1 against QA repo(s)
- `/flow-qa --reset <framework|all>` — reset QA repo(s) to seed state
- `/flow-qa --tier <1|2|3> --framework <name>` — run a specific tier

## QA Repos

Each framework has a dedicated QA repo:

| Framework | Repo | Local path |
|-----------|------|------------|
| python | `benkruger/flow-qa-python` | `.qa-repos/python` |
| rails | `benkruger/flow-qa-rails` | `.qa-repos/rails` |
| ios | `benkruger/flow-qa-ios` | `.qa-repos/ios` |

Create repos with `bin/flow scaffold-qa --framework <name> --repo <owner/repo>`.

## Bare `/flow-qa` (no flags)

### Step 1 — Check setup

Use the Glob tool to check if `.qa-repos/` contains any framework
directories (e.g., `.qa-repos/python/`).

### Step 2 — Report

If `.qa-repos/` contains at least one framework directory, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — READY
──────────────────────────────────────────────────
```
````

Then list which frameworks are set up (which subdirectories exist
under `.qa-repos/`).

If `.qa-repos/` does not exist or is empty, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — NOT SET UP
──────────────────────────────────────────────────
```
````

### Step 3 — Show workflow

After the status banner, print:

> **QA Workflow:**
>
> 1. `/flow-qa --start` — clone and prime QA repo(s)
> 2. `/flow-qa --run <framework|all>` — run Tier 1
> 3. `/flow-qa --reset <framework|all>` — reset QA repos to seed state
> 4. `/flow-qa --stop` — remove local QA repos

## Flag: `--start`

Set up QA repos locally by cloning and priming them. Currently
supports `python` only (rails and ios are out of scope for now).

### Step 1 — Clone QA repo

If `.qa-repos/python` does not already exist, clone it:

```bash
gh repo clone benkruger/flow-qa-python .qa-repos/python
```

If the directory already exists, skip cloning.

### Step 2 — Prime QA repo

Run `prime-setup` directly with fully autonomous settings. This
creates `.flow.json`, `.claude/settings.json`, and all FLOW
artifacts in the QA repo without interactive prompts.

```bash
bin/flow prime-setup .qa-repos/python --framework python --skills-json '{"flow-start":{"continue":"auto"},"flow-plan":{"continue":"auto","dag":"auto"},"flow-code":{"commit":"auto","continue":"auto"},"flow-code-review":{"commit":"auto","continue":"auto","code_review_plugin":"never"},"flow-learn":{"commit":"auto","continue":"auto"},"flow-abort":"auto","flow-complete":"auto"}' --plugin-root $PWD
```

If the JSON output has `"status": "error"`, print the error message and stop.

### Step 3 — Announce

Print inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Setup complete (python)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Flag: `--stop`

Remove the `.qa-repos/` directory to clean up local QA clones.

### Step 1 — Remove QA repos

Use the Bash tool to remove the directory:

```bash
rm -rf .qa-repos
```

### Step 2 — Report

Print inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Cleaned up
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Flag: `--reset <framework|all>`

Reset QA repo(s) to seed state. If `all`, reset all set-up frameworks.

### Step 1 — Determine repos

Map the framework argument to repos. Only reset frameworks that have
a local clone in `.qa-repos/`.

- `python` → `benkruger/flow-qa-python`, local path `.qa-repos/python`
- `rails` → `benkruger/flow-qa-rails`, local path `.qa-repos/rails`
- `ios` → `benkruger/flow-qa-ios`, local path `.qa-repos/ios`
- `all` → all frameworks that have a directory under `.qa-repos/`

### Step 2 — Reset each repo

For each repo, run:

```bash
bin/flow qa-reset --repo <owner/repo> --local-path .qa-repos/<framework>
```

Report the JSON output for each.

### Step 3 — Re-prime each repo

Reset wipes `.flow.json` and `.claude/`, so re-prime each repo:

```bash
bin/flow prime-setup .qa-repos/<framework> --framework <framework> --skills-json '{"flow-start":{"continue":"auto"},"flow-plan":{"continue":"auto","dag":"auto"},"flow-code":{"commit":"auto","continue":"auto"},"flow-code-review":{"commit":"auto","continue":"auto","code_review_plugin":"never"},"flow-learn":{"commit":"auto","continue":"auto"},"flow-abort":"auto","flow-complete":"auto"}' --plugin-root $PWD
```

### Step 4 — Report

Print inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Reset complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Flag: `--run <framework|all>`

Run Tier 1 against the specified framework. If `all`, run against
all set-up frameworks sequentially.

### Step 1 — Verify setup

Check that `.qa-repos/<framework>` exists. If not, tell the user
to run `/flow-qa --start` first and stop.

### Step 2 — Reset and re-prime

Reset the QA repo to seed state:

```bash
bin/flow qa-reset --repo <owner/repo> --local-path .qa-repos/<framework>
```

Re-prime (reset wipes `.flow.json` and `.claude/`):

```bash
bin/flow prime-setup .qa-repos/<framework> --framework <framework> --skills-json '{"flow-start":{"continue":"auto"},"flow-plan":{"continue":"auto","dag":"auto"},"flow-code":{"commit":"auto","continue":"auto"},"flow-code-review":{"commit":"auto","continue":"auto","code_review_plugin":"never"},"flow-learn":{"commit":"auto","continue":"auto"},"flow-abort":"auto","flow-complete":"auto"}' --plugin-root $PWD
```

### Step 3 — Fetch an issue

Get the first open issue from the QA repo:

```bash
gh issue list --repo <owner/repo> --state open --json number --jq '.[0].number'
```

If no issues exist, print "No open issues in QA repo — run
`/flow-qa --reset <framework>` to recreate seed issues" and stop.

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

Run Tier 1 verification:

```bash
bin/flow qa-verify --tier 1 --framework <framework> --repo <owner/repo> --project-root .qa-repos/<framework>
```

Parse the JSON output. Report each check's pass/fail status.

### Step 7 — Report

If all checks passed, print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Tier 1 PASSED (<framework>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If any check failed, print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✗ FLOW QA — Tier 1 FAILED (<framework>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Flag: `--tier <N> --framework <name>`

Run a specific tier against a specific framework.

Map to the corresponding tier definition below and execute its steps.

## Tier Definitions

### Tier 1 — Smoke (Single-Flow Lifecycle)

**Goal:** Verify one complete Start-to-Complete flow works in-session.

Tier 1 is the execution path for `--run`. See the `--run` flag section
above for the full step-by-step procedure.

**Pass criteria:**

- All 6 phases show status `complete`
- PR is merged
- State file is cleaned up after Complete

**Fail criteria:**

- Any phase fails to complete
- PR not created or not merged
- Artifacts left behind after Complete

### Tier 2 — Sequential (Two Autonomous Flows)

**Goal:** Verify two sequential flows on the same repo complete without interference.

**Steps:**

1. Reset the QA repo
2. Start Flow A in-session: `/flow:flow-start --auto fix issue #1`
3. After Flow A completes, start Flow B: `/flow:flow-start --auto fix issue #2`
4. Run verification:

```bash
bin/flow qa-verify --tier 2 --framework <name> --repo <owner/repo> --project-root .qa-repos/<framework>
```

**Pass criteria:**

- Both flows complete all 6 phases
- State files are isolated (different branches)
- No cross-contamination between flows

**Fail criteria:**

- Either flow fails
- State files overlap or corrupt each other
- Lock contention causes timeout

### Tier 3 — Stress (Recovery and Edge Cases)

**Goal:** Verify the system recovers from abnormal conditions.

**Steps:**

1. Reset the QA repo
2. Start a flow, then kill the Claude session mid-Code phase
3. Start a new session and run `/flow:flow-continue` to verify recovery
4. Start two flows sequentially with `--auto` to test lock contention
5. Run cleanup on one flow while another is active
6. Run verification:

```bash
bin/flow qa-verify --tier 3 --framework <name> --repo <owner/repo> --project-root .qa-repos/<framework>
```

**Pass criteria:**

- No stale lock files after all flows complete
- No orphan state files (state without matching worktree)
- Recovery from killed session succeeds

**Fail criteria:**

- Stale lock blocks new flows
- Orphan state files remain
- Recovery fails or loses progress

## Hard Rules

- Never run QA tiers against the FLOW repo itself — only against QA repos in `.qa-repos/`
- Always reset between tier runs to ensure clean state
- Report pass/fail for each tier check individually
- Stop on first tier failure when running all tiers
