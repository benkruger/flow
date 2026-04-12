---
name: flow-prime
description: "One-time project setup — configure and commit workspace permissions, install bin/* stubs, and write the version marker. Run once after installing or upgrading FLOW. Usage: /flow:flow-prime"
---

# FLOW Prime — One-Time Project Setup

## Usage

```text
/flow:flow-prime
/flow:flow-prime --reprime
```

Run once after installing FLOW, and again after each FLOW upgrade. Configures workspace permissions, git excludes, installs the bin/* delegation stubs, and writes a version marker so `/flow:flow-start` knows the project is initialized.

`--reprime` skips all questions and reuses the existing `.flow.json` config. Use this for upgrades where you want the same autonomy and commit format — just new artifacts installed.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — Prime — STARTING
──────────────────────────────────────────────────
```
````

## Reprime Check

If `--reprime` was passed:

1. Use the Read tool to read `.flow.json` from the project root.
   - If the file does not exist, stop with: "No existing config to reprime from. Run `/flow:flow-prime` instead."
2. Extract `skills` and `commit_format` from the JSON.
3. Run `claude plugin list` to check plugin state (needed for Step 4).
4. Skip Steps 1–2 entirely. Jump to Step 3 with the extracted values.

## Steps

### Step 1 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — controls per-task review in phase skills (auto = skip review prompts, manual = require explicit approval before each commit).
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, code-review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (complete, abort) have a single mode value.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-plan": {"continue": "auto", "dag": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": "auto", "flow-abort": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "manual", "dag": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "manual", "continue": "manual"}, "flow-learn": {"commit": "manual", "continue": "manual"}, "flow-complete": "manual", "flow-abort": "manual"}
```

**Recommended** — safe defaults:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "auto", "dag": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": "auto", "flow-abort": "auto"}
```

**Customize** — ask per skill, in this order: start, plan, code, code-review, learn, complete, abort. For each skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

For **start** (continue only), ask one AskUserQuestion:

> "Continue mode for /flow:flow-start?"
>
> - **Manual (Recommended)** — "Prompt before advancing"
> - **Auto** — "Auto-advance to next phase"

For **plan** (continue and dag), ask two AskUserQuestions:

First question:

> "Continue mode for /flow:flow-plan? (controls phase advancement to Code)"
>
> - **Auto (Recommended)** — "Auto-advance to Code phase"
> - **Manual** — "Prompt before advancing"

Second question:

> "DAG mode for /flow:flow-plan? (complexity-aware decomposition via decompose plugin)"
>
> - **Auto (Recommended)** — "Use DAG decomposition for complex features, skip for simple"
> - **Always** — "Always use DAG decomposition"
> - **Never** — "Skip DAG decomposition"

For **code** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-code? (controls per-task review before each commit)"
>
> - **Manual (Recommended)** — "Require explicit approval"
> - **Auto** — "Skip approval prompts"

Second question:

> "Continue mode for /flow:flow-code? (controls phase advancement)"
>
> - **Manual (Recommended)** — "Prompt before advancing"
> - **Auto** — "Auto-advance to next phase"

For **code-review** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-code-review? (controls per-task review before each commit)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-code-review? (controls phase advancement)"
>
> - **Auto (Recommended)** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

For **learning** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-learn? (controls per-task review before each commit)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-learn? (controls phase advancement)"
>
> - **Auto (Recommended)** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

For **complete** and **abort** (single mode), ask one AskUserQuestion each:

> "Mode for /flow:flow-<skill>?"
>
> - **Auto (Recommended)** — "Skip confirmation prompt"
> - **Manual** — "Require confirmation prompt"

Store the result as `skills_dict` for Step 3.

### Step 2 — Choose commit message format

FLOW supports two commit message formats:

- **Title only** — subject line + file list (minimal, no tl;dr section)
- **Full** — subject + tl;dr + explanation + file list (detailed seven-element format)

Ask the user which format to use with AskUserQuestion:

> "What commit message format should FLOW use?"
>
> - **Title only** — "Subject line + file list, no tl;dr section"
> - **Full format** — "Subject + tl;dr + explanation + file list (detailed)"

Store the result as `commit_format`:

- "Title only" → `"title-only"`
- "Full format" → `"full"`

### Step 3 — Run prime setup script

Serialize `skills_dict` from Step 1 as a JSON string for the `--skills-json` argument.
Pass the `commit_format` value from Step 2 via `--commit-format`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --skills-json '<skills_dict_json>' --commit-format <commit_format> --plugin-root ${CLAUDE_PLUGIN_ROOT}
```

The script handles everything in a single call:

- Reading or creating `.claude/settings.json`
- Merging FLOW universal permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker, config hash, skills config, and commit format
- Adding `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, and `.claude/scheduled_tasks.lock` to `.git/info/exclude`
- Installing a pre-commit hook at `.git/hooks/pre-commit` that blocks direct `git commit` during active FLOW features and requires commits to go through `/flow:flow-commit`
- Installing a global `flow` launcher at `~/.local/bin/flow` that delegates to the plugin cache, and warning if `~/.local/bin` is not in PATH
- Installing the bin/* delegation stubs (`bin/format`, `bin/lint`, `bin/build`, `bin/test`) into `<project_root>/bin/` from the FLOW asset templates. Pre-existing `bin/*` scripts are never overwritten — the stubs only fill in the gaps so `bin/flow ci` always has something to call.

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true, "hook_installed": true, "launcher_installed": true, "stubs_installed": ["format", "lint", "build", "test"]}`

If the script returns an error, show the message and stop.

`.flow.json` stores two hashes: `config_hash` (permission structure) and `setup_hash` (entire `prime_setup.rs` file content), both 12-character hex digests. When the plugin version changes, `/flow-start` recomputes both hashes and compares against stored values. If both match, the version is auto-upgraded. If either mismatches, `/flow-prime` must be re-run.

After prime, the user is responsible for editing each `bin/<tool>` to point at their actual toolchain (cargo, pytest, go test, etc.). The default stubs exit 0 with a stderr reminder so a fresh prime never blocks CI.

All universal permissions written to `.claude/settings.json` for reference:

```json
{
  "permissions": {
    "allow": [
      "Bash(git add *)",
      "Bash(git blame *)",
      "Bash(git branch *)",
      "Bash(git config *)",
      "Bash(git -C *)",
      "Bash(git diff *)",
      "Bash(git fetch *)",
      "Bash(git log *)",
      "Bash(git merge *)",
      "Bash(git pull *)",
      "Bash(git push)",
      "Bash(git push *)",
      "Bash(git remote *)",
      "Bash(git reset *)",
      "Bash(git restore *)",
      "Bash(git rev-list *)",
      "Bash(git rev-parse *)",
      "Bash(git show *)",
      "Bash(git status)",
      "Bash(git symbolic-ref *)",
      "Bash(git worktree *)",
      "Bash(cd *)",
      "Bash(pwd)",
      "Bash(chmod +x *)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(gh pr list *)",
      "Bash(gh pr view *)",
      "Bash(gh pr checks *)",
      "Bash(gh pr merge *)",
      "Bash(gh issue *)",
      "Bash(gh label *)",
      "Bash(gh -C *)",
      "Bash(*bin/flow *)",
      "Bash(rm .flow-*)",
      "Bash(test -f *)",
      "Bash(claude plugin list)",
      "Bash(claude plugin marketplace add *)",
      "Bash(claude plugin install *)",
      "Bash(curl *)",
      "Read(~/.claude/rules/*)",
      "Read(~/.claude/projects/*/memory/*)",
      "Read(//tmp/*.txt)",
      "Read(//tmp/*.diff)",
      "Read(//tmp/*.patch)",
      "Read(//tmp/*.md)",
      "Agent(flow:ci-fixer)",
      "Skill(decompose:decompose)"
    ],
    "deny": [
      "Bash(git rebase *)",
      "Bash(git push --force *)",
      "Bash(git push -f *)",
      "Bash(git reset --hard *)",
      "Bash(git stash *)",
      "Bash(git checkout *)",
      "Bash(git clean *)",
      "Bash(git commit *)",
      "Bash(gh pr merge * --admin*)",
      "Bash(gh * --admin*)",
      "Bash(cargo *)",
      "Bash(rustc *)",
      "Bash(go *)",
      "Bash(bundle *)",
      "Bash(rubocop *)",
      "Bash(ruby *)",
      "Bash(rails *)",
      "Bash(xcodebuild *)",
      "Bash(xcrun *)",
      "Bash(swift *)",
      "Bash(swiftlint *)",
      "Bash(.venv/bin/*)",
      "Bash(python3 -m pytest *)",
      "Bash(pytest *)",
      "Bash(npm *)",
      "Bash(npx *)",
      "Bash(yarn *)",
      "Bash(pnpm *)",
      "Bash(gradle *)",
      "Bash(gradlew *)",
      "Bash(./gradlew *)",
      "Bash(mvn *)",
      "Bash(./mvnw *)",
      "Bash(mix *)",
      "Bash(elixir *)",
      "Bash(dotnet *)",
      "Bash(* && *)",
      "Bash(* ; *)",
      "Bash(* | *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 4 — Install plugins

Run `claude plugin list` to check the current plugin state.

**Decompose plugin (DAG planning):**

If the output does not contain `decompose-marketplace`, add the marketplace source:

```bash
claude plugin marketplace add matt-k-wong/mkw-DAG-architect
```

If the output does not contain `decompose`, install it:

```bash
claude plugin install decompose@decompose-marketplace
```

If all plugins are already present, skip silently.

### Step 5 — Commit generated files

Check if the working tree has changes by running `git status`. If the output contains "working tree clean", skip to Done.

Otherwise, invoke `/flow:flow-commit` to commit and push the generated files (`.claude/settings.json` and any newly-installed `bin/<tool>` stubs).

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Prime — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Report:

- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json` (git-excluded)
- Git excludes configured for `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, and `.claude/scheduled_tasks.lock`
- Pre-commit hook installed — blocks direct `git commit`, requires `/flow:flow-commit`
- Global launcher installed at `~/.local/bin/flow` — run `flow tui` from any primed project
- bin/* stubs installed (list whichever names appear in `stubs_installed` from Step 3); remind the user to edit each one to wire it to their actual toolchain
- Slack notifications: configured via plugin userConfig (token in system keychain)
- Generated files committed and pushed

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start       | —      | manual   |
| plan        | —      | auto     |
| code        | manual | manual   |
| code-review | auto   | auto     |
| learning    | auto   | auto     |
| complete    | auto   | —        |
| abort       | auto   | —        |
```

Use the actual values from `skills_dict` (Step 1). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow-start <feature name>`.
