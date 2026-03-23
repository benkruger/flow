---
name: flow-qa
description: "QA the FLOW plugin locally. Switch to local plugin source for testing, restore when done. Run tiered QA against per-framework repos."
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

### Step 1 — Check dev mode

Use the Read tool to read `.flow.json`. Parse the JSON and check if the
`plugin_root_backup` key exists. If `.flow.json` does not exist or cannot
be parsed, treat as marketplace mode.

### Step 2 — Report

If `plugin_root_backup` exists in the JSON, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE (local)
──────────────────────────────────────────────────
```
````

If `plugin_root_backup` does not exist, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — MARKETPLACE (remote)
──────────────────────────────────────────────────
```
````

### Step 3 — Show workflow

After the mode banner, print:

> **QA Workflow:**
>
> 1. `/flow-qa --start` — redirect plugin_root to local source
> 2. Start Claude with `claude --plugin-dir=$HOME/code/flow`
> 3. `/flow-qa --run <framework|all>` — run all tiers
> 4. `/flow-qa --tier <N> --framework <name>` — run a specific tier
> 5. `/flow-qa --reset <framework|all>` — reset QA repos to seed state
> 6. `/flow-qa --stop` — restore marketplace plugin

## Flag: `--start`

### Step 1 — Redirect plugin_root to local source

Run:

```bash
bin/flow qa-mode --start --local-path $HOME/code/flow
```

If the JSON output has `"status": "error"`, print the error message and stop.

### Step 2 — Announce

Print inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE ACTIVE
──────────────────────────────────────────────────
```
````

Then print these numbered instructions:

> **Next steps:**
>
> 1. Run `/reload-plugins` now to update the skill list for this session.
> 2. Start a new Claude Code session with `claude --plugin-dir=$HOME/code/flow`
>    to load local source as the plugin.
> 3. Run `/flow-qa --run <framework|all>` or `/flow-qa --tier <N> --framework <name>`
>    to execute QA tiers.
> 4. When done, run `/flow-qa --stop` to restore the marketplace plugin.

## Flag: `--stop`

### Step 1 — Restore plugin_root from backup

Run:

```bash
bin/flow qa-mode --stop
```

If the JSON output has `"status": "error"`, print the error message and stop.

### Step 2 — Report

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
