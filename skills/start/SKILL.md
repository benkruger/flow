---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades dependencies, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:start <feature name words>"
model: haiku
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:start invoice pdf export
```

Arguments become the feature name. Words are joined with hyphens:

- Branch: `invoice-pdf-export`
- Worktree: `.worktrees/invoice-pdf-export`
- PR title: `Invoice Pdf Export`

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen.

<HARD-GATE>
Do NOT proceed if the feature name is missing. Ask the user:
"What is the feature name? e.g. /flow:start invoice pdf export"
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.15.0 — Phase 1: Start — STARTING
============================================
```
````

## Logging

After every Bash command in Steps 5–8, log it to `.flow-states/<branch>.log`. Step 4 handles its own logging internally.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 1] Step X — desc (exit EC)
```

Use the feature name as `<branch>` — it matches the branch name.

---

## Steps

### Step 1 — Check for existing feature

Use the Glob tool to check for existing state files matching `.flow-states/*.json`.

If any files are found, list their names (the branch names from the filenames).

If any files are found, use AskUserQuestion:

> "An active FLOW feature already exists. What would you like to do?"
>
> - **Start a new feature anyway** — proceed
> - **Cancel** — stop here

### Step 2 — Version gate

Run the version check before any slow operations:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow init-check
```

Parse the JSON output. If `"status": "error"`, tell the user to run `/flow:init` and stop. Do not proceed to any further steps.

### Step 3 — Verify main is green

Run `bin/flow ci` on main before creating any resources:

```bash
bin/flow ci
```

If it fails, stop immediately:

> "`bin/flow ci` is failing on main. Please fix CI before starting a new feature."

Do not create a worktree, PR, or state file. Exit the skill entirely.

### Step 4 — Set up workspace

Run the consolidated setup script:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>"
```

The script performs these operations in a single process:

1. `git pull origin main`
2. `git worktree add .worktrees/<branch> -b <branch>`
3. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
4. Create `.flow-states/<branch>.json` (initial state, all 8 phases)

The script logs each operation to `.flow-states/<branch>.log` internally.

**On success** — stdout is JSON:

```json
{"status": "ok", "worktree": ".worktrees/<branch>", "pr_url": "...", "pr_number": 123, "feature": "...", "branch": "..."}
```

Parse the JSON. Then cd into the worktree:

```bash
cd .worktrees/<branch>
```

The Bash tool persists working directory between calls, so all subsequent
commands run inside the worktree automatically. Do NOT repeat `cd .worktrees/`
in later steps — it would look for a nested `.worktrees/` that doesn't exist.

**On failure** — stdout is error JSON, details on stderr:

```json
{"status": "error", "step": "git_pull", "message": "..."}
```

If the script returns an error, read the stderr output for details, report
the failure to the user, and stop.

Read the `framework` field from the state file (`.flow-states/<branch>.json`)
and follow only the matching section below. Do not announce the framework —
just follow the matching section silently.

#### If Rails

### Step 5 — Upgrade gems

```bash
bundle update --all
```

### Step 6 — Post-upgrade `bin/flow ci`

```bash
bin/flow ci
```

- **Passes** — continue to Step 8
- **Fails** — launch the CI fix sub-agent (see Step 7). Pass the full
  `bin/flow ci` output. After the sub-agent returns:
  - **Fixed** — continue to Step 8 (Gemfile.lock + fixes committed together)
  - **Not fixed** — stop and report to the user what is failing

### Step 7 — CI fix sub-agent

When `bin/flow ci` fails in Step 6, launch a sub-agent to diagnose
and fix the failures. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `model`: `"sonnet"`
- `description`: `"Fix bin/flow ci failures"`

Provide these instructions (fill in the worktree path and bin/flow ci output):

> You are fixing CI failures in a Rails worktree.
> Worktree: `<worktree path>`
> cd into the worktree before running any commands.
>
> The `bin/flow ci` output:
> <paste the full bin/flow ci output>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions (bin/flow ci, bin/rails test,
> rubocop -A, rubocop). Never use Bash for any other purpose — no find,
> ls, cat, wc, test -f, stat, or running project tooling not listed here.
>
> Fix the failures in this order:
>
> 1. **RuboCop violations** — ALWAYS run `rubocop -A` first. This
>    auto-corrects most violations. Then run `bin/flow ci`. If violations
>    remain, fix the code manually to satisfy the cop.
> 2. **Test failures** — read the failing test and the code it tests.
>    Understand the root cause. Fix the code, not the test (unless the
>    test itself is wrong). Run `bin/rails test <file>` to verify,
>    then `bin/flow ci` for a full check.
> 3. **Coverage gaps** — read `test/coverage/uncovered.txt` to see exactly
>    which lines are uncovered. Write the missing test, then `bin/flow ci`
>
> **Never modify `.rubocop.yml` or any RuboCop configuration.**
> Fix the code, never the rules. Do not add exclusions or disable cops.
>
> Max 3 attempts. After each fix, run `bin/flow ci`. If green, report what
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
Do NOT proceed past Step 6 until `bin/flow ci` is green.
</HARD-GATE>

### Step 8 — Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` + any gem fixes).

#### Rails report additions

Include in the Done report:

- Which gems were upgraded (`git diff Gemfile.lock` summary)
- Confirmation `bin/flow ci` is green

#### If Python

No additional setup needed — Step 3 already verified `bin/flow ci` on main,
and Python has no dependency upgrade step. Proceed silently to Done —
do not announce the framework or explain why steps were skipped.

### Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase 1 --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.15.0 — Phase 1: Start — COMPLETE (<formatted_time>)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Plan?"
>
> - **Yes, start Phase 2 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 2 now" and "Not yet"

**If Yes** — invoke the `flow:plan` skill using the Skill tool.

**If Not yet** — print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

Then report:
- Worktree location
- PR link
- Any additional report items from the framework section above

## Hard Rules

- Do not narrate internal operations to the user — no "The framework is Python", no "Proceeding to phase completion", no "No additional setup steps are needed". Just do the work silently and show results
