---
name: start
description: "Phase 1: Start — begin a new feature. Creates a worktree, upgrades gems, opens a PR, creates .flow-states/<branch>.json, and configures the workspace. Usage: /flow:start <feature name words>"
model: haiku
---

# FLOW Start — Phase 1: Start

## Usage

```text
/flow:start invoice pdf export
/flow:start --light fix login bug
```

Arguments become the feature name. Words are joined with hyphens:

- Branch: `invoice-pdf-export`
- Worktree: `.worktrees/invoice-pdf-export`
- PR title: `Invoice Pdf Export`

Branch names are capped at **32 characters**. If the hyphenated name exceeds 32 characters, truncate at the last whole word (hyphen boundary) that fits. Strip any trailing hyphen.

**`--light` flag:** For bug fixes and small changes that don't need full Design
ceremony. When present, strip `--light` from the feature name before deriving
the branch. Design (Phase 3) is marked complete+skipped in the state file.
Research writes a simplified design object so Plan and Review work unchanged.
Pass `--light` to `start-setup` as a separate argument after the feature name.

<HARD-GATE>
Do NOT proceed if the feature name is missing. Ask the user:
"What is the feature name? e.g. /flow:start invoice pdf export"
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.12.0 — Phase 1: Start — STARTING
============================================
```
````

## Logging

After every Bash command in Steps 3–7, log it to `.flow-states/<branch>.log`. Step 2 handles its own logging internally.

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

### Step 2 — Set up workspace

Run the consolidated setup script. If `--light` was specified, pass it as a
second argument:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>"
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow start-setup "<feature-name>" --light
```

The script performs these operations in a single process:

1. Verify `/flow:init` has been run (version gate)
2. `git pull origin main`
3. `git worktree add .worktrees/<branch> -b <branch>`
4. `git commit --allow-empty` + `git push -u origin` + `gh pr create`
5. Create `.flow-states/<branch>.json` (initial state, all 9 phases)

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

If the error step is `init_check`, tell the user to run `/flow:init` first
and stop. For all other errors, read the stderr output for details, report
the failure to the user, and stop.

### Step 3 — Framework-specific setup

Read the framework-specific instructions from
`${CLAUDE_PLUGIN_ROOT}/skills/start/<framework>.md`
where `<framework>` is the `framework` field from the state file
(`.flow-states/<branch>.json`).

Follow all steps in the fragment before proceeding to Done.

### Done — Update state and complete phase

Update `.flow-states/<branch>.json`:
1. `cumulative_seconds` for Phase 1: `current_time - session_started_at`. Do not print the calculation.
2. Phase 1 `status` → `complete`
3. Phase 1 `completed_at` → current UTC timestamp
4. Phase 1 `session_started_at` → `null`
5. `current_phase` → `2`

Update Phase 1 task to `completed`.

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.12.0 — Phase 1: Start — COMPLETE (<formatted_time>)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 1: Start is complete. Ready to begin Phase 2: Research?"
>
> - **Yes, start Phase 2 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 2 now" and "Not yet"

**If Yes** — invoke the `flow:research` skill using the Skill tool.

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
- Any additional report items from the framework fragment
