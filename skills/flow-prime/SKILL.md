---
name: flow-prime
description: "One-time project setup — configure workspace permissions, git excludes, and version marker. Run once after installing or upgrading FLOW. Usage: /flow:flow-prime"
---

# FLOW Prime — One-Time Project Setup

## Usage

```text
/flow:flow-prime
/flow:flow-prime --reprime
```

Run once after installing FLOW, and again after each FLOW upgrade. Configures workspace permissions, git excludes, and writes a version marker so `/flow:flow-start` knows the project is initialized.

`--reprime` skips all questions and reuses the existing `.flow.json` config. Use this for upgrades where you want the same framework, autonomy, and commit format — just new artifacts installed.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — Prime — STARTING
──────────────────────────────────────────────────
```
````

## Reprime Check

If `--reprime` was passed:

1. Use the Read tool to read `.flow.json` from the project root.
   - If the file does not exist, stop with: "No existing config to reprime from. Run `/flow:flow-prime` instead."
2. Extract `framework`, `skills`, and `commit_format` from the JSON.
3. Run `claude plugin list` to check plugin state (needed for Step 6).
4. Skip Steps 1–4 entirely. Jump to Step 5 with the extracted values.

## Steps

### Step 1 — Detect framework and check plugins

Run both in parallel (one response, two Bash calls):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow detect-framework <project_root>
```

```bash
claude plugin list
```

Keep the plugin list output for Step 6 — do not re-run it.

Parse the detect-framework JSON output. The `detected` array contains frameworks matched
by file presence, and `available` lists all supported frameworks.

If exactly one framework is detected, confirm with AskUserQuestion:

> "Detected **<display_name>** project. Is this correct?"
>
> - **Yes, <display_name>** — "Proceed with <display_name> setup"
> - One option per other available framework — "<display_name>"

If no frameworks detected, or multiple detected, ask the user to choose
from the available list using AskUserQuestion with one option per
available framework.

Store the answer as `framework` (lowercase name from the JSON).

### Step 2 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — how `/flow:flow-commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code.
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, code-review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (abort, complete) have a single mode value. The `/flow:flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-plan": {"continue": "auto", "dag": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-complete": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "manual", "dag": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "manual", "continue": "manual"}, "flow-learn": {"commit": "manual", "continue": "manual"}, "flow-abort": "manual", "flow-complete": "manual"}
```

**Recommended** — safe defaults for all frameworks:

```json
{"flow-start": {"continue": "manual"}, "flow-plan": {"continue": "auto", "dag": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-code-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-abort": "auto", "flow-complete": "auto"}
```

**Customize** — ask per skill, in this order: start, plan, code, code-review, learn, abort, complete. For each skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

For **code** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-code? (controls diff approval and per-task approval)"
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

> "Commit mode for /flow:flow-code-review? (controls diff approval and per-task approval)"
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

> "Commit mode for /flow:flow-learn? (controls diff approval and per-task approval)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-learn? (controls phase advancement)"
>
> - **Auto (Recommended)** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

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

For **abort** and **complete** (single mode), ask one AskUserQuestion each:

> "Mode for /flow:flow-<skill>?"
>
> - **Auto (Recommended)** — "Skip confirmation prompt"
> - **Manual** — "Require confirmation prompt"

Store the result as `skills_dict` for Step 5.

### Step 3 — Choose code review plugin mode

The code-review plugin provides multi-agent validation as Step 4 of the Code Review phase. Its mode controls whether that step runs.

Ask the user which mode to use with AskUserQuestion:

> "Code Review Plugin mode? (multi-agent validation via code-review:code-review)"
>
> - **Always (Default)** — "Always run the code-review plugin step"
> - **Auto** — "Run plugin based on change complexity"
> - **Never** — "Skip the code-review plugin step"

Store the result by injecting into `skills_dict["flow-code-review"]["code_review_plugin"]`:

- "Always" → `"always"`
- "Auto" → `"auto"`
- "Never" → `"never"`

### Step 4 — Choose commit message format

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

### Step 5 — Run prime setup script

Serialize `skills_dict` from Step 2 as a JSON string for the `--skills-json` argument.
Pass the `commit_format` value from Step 4 via `--commit-format`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --framework <framework> --skills-json '<skills_dict_json>' --commit-format <commit_format> --plugin-root ${CLAUDE_PLUGIN_ROOT}
```

The script handles everything in a single call:

- Reading or creating `.claude/settings.json`
- Merging FLOW permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker, framework, config hash, skills config, and commit format
- Adding `.flow-states/`, `.worktrees/`, `.flow.json`, `bin/dependencies`, and `.claude/scheduled_tasks.lock` to `.git/info/exclude`
- Installing a pre-commit hook at `.git/hooks/pre-commit` that blocks direct `git commit` during active FLOW features and requires commits to go through `/flow:flow-commit`
- Configuring Slack notifications: if `FLOW_SLACK_TOKEN` and `FLOW_SLACK_CHANNEL` env vars are both set, writes `slack.bot_token` and `slack.channel` to `.flow.json` and sets `notify` to `auto`; validates the token with Slack `auth.test` (warns on failure, does not block); if either env var is absent, removes the `slack` key and sets `notify` to `never`
- Installing a global `flow` launcher at `~/.local/bin/flow` that delegates to the plugin cache, and warning if `~/.local/bin` is not in PATH
- Priming the project CLAUDE.md with framework conventions (if CLAUDE.md exists)
- Creating `bin/dependencies` from the framework template (skips if already exists)

Output JSON: `{"status": "ok", "settings_merged": true, "exclude_updated": true, "version_marker": true, "hook_installed": true, "launcher_installed": true, "framework": "rails|python|ios", "prime_project": "ok|error", "dependencies": "ok|skipped"}`

If the script returns an error, show the message and stop.

`.flow.json` stores two hashes: `config_hash` (permission structure) and `setup_hash` (entire `prime-setup.py` file content), both 12-character hex digests. When the plugin version changes, `/flow-start` recomputes both hashes and compares against stored values. If both match, the version is auto-upgraded. If either mismatches, `/flow-prime` must be re-run.

The permissions merged depend on the framework. Universal permissions are
always merged. Framework-specific permissions are loaded from
`frameworks/<name>/permissions.json` and added based on the chosen framework.

All permissions (universal + all framework sets) for reference:

```json
{
  "permissions": {
    "allow": [
      "Bash(git add *)",
      "Bash(git blame *)",
      "Bash(git branch *)",
      "Bash(git commit *)",
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
      "Bash(bin/*)",
      "Bash(*bin/flow *)",
      "Bash(rm .flow-*)",
      "Bash(gh pr view *)",
      "Bash(bin/rails test *)",
      "Bash(rails *)",
      "Bash(RAILS_ENV=* rails *)",
      "Bash(RAILS_ENV=* bundle *)",
      "Bash(RAILS_ENV=* bin/*)",
      "Bash(bundle *)",
      "Bash(ruby *)",
      "Bash(./migrate.sh *)",
      "Bash(rubocop *)",
      "Bash(rubocop -A)",
      "Bash(psql *)",
      "Bash(.venv/bin/pip install *)",
      "Bash(open *)",
      "Bash(xcrun *)",
      "Bash(xcodebuild *)",
      "Bash(gh pr checks *)",
      "Bash(gh pr merge *)",
      "Bash(claude plugin list)",
      "Bash(claude plugin marketplace add *)",
      "Bash(claude plugin install *)",
      "Bash(gh issue *)",
      "Bash(gh label *)",
      "Bash(curl *)",
      "Read(~/.claude/rules/*)",
      "Read(//tmp/*.txt)",
      "Read(//tmp/*.diff)",
      "Read(//tmp/*.patch)",
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
      "Bash(* && *)",
      "Bash(* ; *)",
      "Bash(* | *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 6 — Install plugins

Use the `claude plugin list` output from Step 1 (do not re-run it).

**Code-review plugin:**

If the output does not contain `claude-code-plugins`, add the marketplace source:

```bash
claude plugin marketplace add anthropics/claude-code
```

If the output does not contain `code-review`, install it:

```bash
claude plugin install code-review@claude-code-plugins
```

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

### Step 7 — Commit or git-exclude

Check if the working tree has changes by running `git status`. If the output contains "working tree clean", skip to Done.

Otherwise, the user decides whether to commit the generated files into the repo or keep them local-only.

<HARD-GATE>
Do NOT commit, exclude, or take any action without an explicit choice from the user. Use AskUserQuestion:

> "FLOW generated `CLAUDE.md` and `.claude/settings.json`. How should these be handled?"
>
> - **Commit and push** — "Track in version control (shared with team)"
> - **Git-exclude (keep local-only)** — "Add to .git/info/exclude (local to this clone)"

Do NOT proceed until the user responds.

**If "Commit and push":** invoke `/flow:flow-commit`.

**If "Git-exclude (keep local-only)":** use the Read tool to read `.git/info/exclude`. Check whether `CLAUDE.md` and `.claude/` are already present. For each entry that is missing, use the Edit tool to append it. Do NOT invoke `/flow:flow-commit`.

</HARD-GATE>

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — Prime — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Report:

- Framework: `<framework>`
- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json` (git-excluded)
- Git excludes configured for `.flow-states/`, `.worktrees/`, `.flow.json`, `bin/dependencies`, and `.claude/scheduled_tasks.lock`
- Pre-commit hook installed — blocks direct `git commit`, requires `/flow:flow-commit`
- Global launcher installed at `~/.local/bin/flow` — run `flow tui` from any primed project
- Slack notifications: configured (if env vars set) or disabled (if absent)
- If the user chose "Commit and push": Changes committed and pushed
- If the user chose "Git-exclude": Generated files git-excluded (local-only) — `CLAUDE.md` and `.claude/` kept local to this clone

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start       | —      | manual   |
| plan        | —      | auto     |
| code        | manual | manual   |
| code-review | auto   | auto     |
| learning    | auto   | auto     |
| abort       | auto   | —        |
| complete    | auto   | —        |
```

Use the actual values from `skills_dict` (Step 2). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.

Tell the user to start a new Claude Code session so the permissions take effect, then run `/flow-start <feature name>`.
