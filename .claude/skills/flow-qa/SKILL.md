---
name: flow-qa
description: "QA the FLOW plugin locally. Uninstall marketplace plugin for local testing, reinstall when done. Run tiered QA against per-framework repos."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — requires
the plugin to be installed.

## Usage

```text
/flow-qa
/flow-qa --start
/flow-qa --stop
/flow-qa --run <framework|all>
/flow-qa --reset <framework|all>
/flow-qa --tier <1|2|3> --framework <name>
```

- `/flow-qa` — show current mode (dev or marketplace)
- `/flow-qa --start` — switch to dev mode (local plugin source)
- `/flow-qa --stop` — switch back to marketplace plugin
- `/flow-qa --run <framework|all>` — run all 3 tiers against QA repo(s)
- `/flow-qa --reset <framework|all>` — reset QA repo(s) to seed state
- `/flow-qa --tier <1|2|3> --framework <name>` — run a specific tier

## QA Repos

Each framework has a dedicated QA repo:

| Framework | Repo |
|-----------|------|
| rails | `benkruger/flow-qa-rails` |
| python | `benkruger/flow-qa-python` |
| ios | `benkruger/flow-qa-ios` |

Create repos with `bin/flow scaffold-qa --framework <name> --repo <owner/repo>`.
Reset with `bin/flow qa-reset --repo <owner/repo>`.

## Bare `/flow-qa` (no flags)

### Step 1 — Check dev mode marker

Use the Read tool to check if `.flow-states/.dev-mode` exists.

### Step 2 — Report

If `.flow-states/.dev-mode` exists, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE (local)
──────────────────────────────────────────────────
```
````

If `.flow-states/.dev-mode` does not exist, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — MARKETPLACE (remote)
──────────────────────────────────────────────────
```
````

## Flag: `--start`

### Step 1 — Check if marketplace plugin is installed

Run:

```bash
claude plugin list
```

If the output contains `flow@flow-marketplace`, run:

```bash
claude plugin uninstall flow@flow-marketplace
```

If the output does not contain `flow@flow-marketplace`, print "Marketplace
plugin not installed, nothing to uninstall." and continue.

### Step 2 — Nuke cache

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

### Step 3 — Create dev mode marker

Use the Write tool to create `.flow-states/.dev-mode` with the content `active`.

### Step 4 — Announce

Print inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE ACTIVE
──────────────────────────────────────────────────
```
````

Then print:

> To test local source, start Claude Code with:
>
> `claude --plugin-dir=$HOME/code/flow`
>
> Run `/flow-qa --stop` when done to reinstall the marketplace plugin.

Then tell the user:

> Run `/reload-plugins` now to update the skill list for this session.

## Flag: `--stop`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it does not exist, print "Not in dev mode. Nothing to stop." and stop.

### Step 2 — Nuke cache and reinstall marketplace plugin

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin install flow@flow-marketplace
```

### Step 3 — Remove dev mode marker

Use Bash to remove the marker:

```bash
rm .flow-states/.dev-mode
```

### Step 4 — Report

Print inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Dev mode stopped
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then tell the user:

> Run `/reload-plugins` now to update the skill list for this session.

## Flag: `--reset <framework|all>`

Reset QA repo(s) to seed state. If `all`, reset all 3 frameworks.

### Step 1 — Determine repos

Map the framework argument to repos:

- `rails` → `benkruger/flow-qa-rails`
- `python` → `benkruger/flow-qa-python`
- `ios` → `benkruger/flow-qa-ios`
- `all` → all three repos

### Step 2 — Reset each repo

For each repo, run:

```bash
bin/flow qa-reset --repo <owner/repo>
```

Report the JSON output for each.

### Step 3 — Report

Print inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Reset complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Flag: `--run <framework|all>`

Run all 3 tiers against the specified framework(s). If `all`, run
against all 3 frameworks sequentially.

For each framework, run Tier 1, then Tier 2, then Tier 3. Stop on
the first tier failure.

## Flag: `--tier <N> --framework <name>`

Run a specific tier against a specific framework.

## Tier Definitions

### Tier 1 — Smoke (Single-Flow Lifecycle)

**Goal:** Verify one complete Start-to-Complete flow works.

**Steps:**

1. Clone the QA repo locally if not already cloned
2. Run `/flow:flow-prime` in the QA repo
3. Run `/flow:flow-start fix issue #1` in the QA repo
4. Complete all 6 phases manually (Plan through Complete)
5. Run verification:

```bash
bin/flow qa-verify --tier 1 --framework <name> --repo <owner/repo> --project-root <path>
```

**Pass criteria:**

- All 6 phases show status `complete`
- PR is merged
- State file is cleaned up after Complete

**Fail criteria:**

- Any phase fails to complete
- PR not created or not merged
- Artifacts left behind after Complete

### Tier 2 — Concurrent (Two Autonomous Flows)

**Goal:** Verify two flows run simultaneously without interference.

**Steps:**

1. Reset the QA repo: `bin/flow qa-reset --repo <owner/repo>`
2. Start Flow A: `/flow:flow-start --auto fix issue #1`
3. While Flow A runs, start Flow B in a separate Claude session:
   `claude -p "run /flow:flow-start --auto fix issue #2" --plugin-dir=$HOME/code/flow`
   If the background session cannot be launched, instruct the user to
   open a second terminal and run the command manually.
4. Wait for both flows to complete
5. Run verification:

```bash
bin/flow qa-verify --tier 2 --framework <name> --repo <owner/repo> --project-root <path>
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
4. Start two flows simultaneously with `--auto` to test lock contention
5. Run cleanup on one flow while another is active
6. Run verification:

```bash
bin/flow qa-verify --tier 3 --framework <name> --repo <owner/repo> --project-root <path>
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

- Never run QA tiers against the FLOW repo itself — only against QA repos
- Always reset between tier runs to ensure clean state
- Report pass/fail for each tier check individually
- Stop on first tier failure when running all tiers
