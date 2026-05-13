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
  FLOW v2.0.1 — Prime — STARTING
──────────────────────────────────────────────────
```
````

## Reprime Check

If `--reprime` was passed:

1. Use the Read tool to read `.flow.json` from the project root.
   - If the file does not exist, stop with: "No existing config to reprime from. Run `/flow:flow-prime` instead."
2. Extract `skills`, `commit_format`, and `role` from the JSON. The
   `role` field is optional — `.flow.json` files written before the
   role-selection step omit it, in which case treat it as unset and
   omit `--role` from the setup-script call.
3. Run `claude plugin list` to check plugin state (needed for Step 5).
4. Skip Steps 1–3 entirely. Jump to Step 4 with the extracted values.

## Steps

### Step 1 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — controls per-task review in phase skills (auto = skip review prompts, manual = require explicit approval before each commit).
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (complete, abort) have a single mode value.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": "auto", "flow-abort": "auto"}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-review": {"commit": "manual", "continue": "manual"}, "flow-learn": {"commit": "manual", "continue": "manual"}, "flow-complete": "manual", "flow-abort": "manual"}
```

**Recommended** — safe defaults:

```json
{"flow-start": {"continue": "manual"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": "auto", "flow-abort": "auto"}
```

**Customize** — ask per skill, in this order: start, code, review, learn, complete, abort. For each skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

For **start** (continue only), ask one AskUserQuestion:

> "Continue mode for /flow:flow-start?"
>
> - **Manual (Recommended)** — "Prompt before advancing"
> - **Auto** — "Auto-advance to next phase"

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

For **review** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-review? (controls per-task review before each commit)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-review? (controls phase advancement)"
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

Store the result as `skills_dict` for Step 4.

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

### Step 3 — Choose primary role

The user's primary role sets a default planning persona for future
planning conversations — a PM user gets the Tech Lead voice by
default, a Tech Lead user gets the PM voice, founder/solo users wear
multiple hats and get no default. The field is optional; skipping
records no default and lets each conversation choose its own persona.

<HARD-GATE>
You MUST ask the user with AskUserQuestion below and wait for an
explicit selection before proceeding. Do NOT infer the answer from
context, do NOT default to any option, and do NOT skip the prompt
even when reprime appears to already carry a role. The role choice
is a user decision per `.claude/rules/skill-authoring.md` "Decision
Point Gates".

> "What is your primary role? (Optional — sets a default planning persona.)"
>
> - **PM** — Product / project management role
> - **Tech Lead** — Engineering lead role
> - **Founder / Solo Dev** — Wear multiple hats
> - **Skip** — No default; choose per conversation

</HARD-GATE>

Store the result as `role_value`:

- "PM" → `"pm"`
- "Tech Lead" → `"tech-lead"`
- "Founder / Solo Dev" → `"founder-solo"`
- "Skip" → omit `--role` from the setup-script call below

### Step 4 — Run prime setup script

Serialize `skills_dict` from Step 1 as a JSON string for the `--skills-json` argument.
Pass the `commit_format` value from Step 2 via `--commit-format`.

The `role_value` from Step 3 maps to one of two invocation shapes
below. **Do not pass the literal string `<role_value>` or `skip`
as the flag argument** — the role-selection step either yielded a
concrete value (pm, tech-lead, founder-solo) OR the user chose
Skip, and the two shapes diverge.

When the user selected a concrete role (PM, Tech Lead, or Founder /
Solo Dev), include `--role <role_value>`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --skills-json '<skills_dict_json>' --commit-format <commit_format> --role <role_value> --plugin-root ${CLAUDE_PLUGIN_ROOT}
```

When the user chose Skip, omit `--role` entirely:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --skills-json '<skills_dict_json>' --commit-format <commit_format> --plugin-root ${CLAUDE_PLUGIN_ROOT}
```

The script handles everything in a single call:

- Reading or creating `.claude/settings.json`
- Merging FLOW universal permissions (additive only — preserves existing entries)
- Setting `defaultMode` to `acceptEdits` (overrides existing values — FLOW requires this for state file writes without prompts)
- Writing `.flow.json` with version marker, config hash, skills config, and commit format
- Adding `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, `.claude/scheduled_tasks.lock`, `test_adversarial_flow.*`, `adversarial_flow_test.go`, `adversarial_flow_test.rb`, `adversarial_flow_spec.rb`, and `AdversarialFlowTests.swift` to `.git/info/exclude`
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
      "Bash(git cat-file *)",
      "Bash(git -C *)",
      "Bash(git diff *)",
      "Bash(git fetch *)",
      "Bash(git for-each-ref *)",
      "Bash(git grep *)",
      "Bash(git log *)",
      "Bash(git ls-files *)",
      "Bash(git ls-tree *)",
      "Bash(git merge *)",
      "Bash(git pull *)",
      "Bash(git push)",
      "Bash(git push *)",
      "Bash(git remote *)",
      "Bash(git restore *)",
      "Bash(git rev-list *)",
      "Bash(git rev-parse *)",
      "Bash(git rm *)",
      "Bash(git show *)",
      "Bash(git status)",
      "Bash(git status *)",
      "Bash(git symbolic-ref *)",
      "Bash(git worktree *)",
      "Bash(cd *)",
      "Bash(pwd)",
      "Bash(chmod +x *)",
      "Bash(awk *)",
      "Bash(bash -n *)",
      "Bash(cat *)",
      "Bash(cmp *)",
      "Bash(command -v *)",
      "Bash(cut *)",
      "Bash(date)",
      "Bash(date *)",
      "Bash(diff *)",
      "Bash(file *)",
      "Bash(find *)",
      "Bash(grep *)",
      "Bash(head *)",
      "Bash(id)",
      "Bash(jq *)",
      "Bash(ls *)",
      "Bash(mkdir *)",
      "Bash(mktemp)",
      "Bash(mktemp *)",
      "Bash(psql *)",
      "Bash(rg *)",
      "Bash(sed *)",
      "Bash(shellcheck *)",
      "Bash(sort *)",
      "Bash(stat *)",
      "Bash(tail *)",
      "Bash(test -d *)",
      "Bash(touch *)",
      "Bash(tr *)",
      "Bash(uname *)",
      "Bash(uniq *)",
      "Bash(wc *)",
      "Bash(which *)",
      "Bash(whoami)",
      "Bash(gh pr create *)",
      "Bash(gh pr edit *)",
      "Bash(gh pr close *)",
      "Bash(gh pr list *)",
      "Bash(gh pr view *)",
      "Bash(gh pr checks *)",
      "Bash(gh pr merge *)",
      "Bash(gh pr comment *)",
      "Bash(gh pr diff *)",
      "Bash(gh pr ready *)",
      "Bash(gh pr reopen *)",
      "Bash(gh pr review *)",
      "Bash(gh pr status *)",
      "Bash(gh issue *)",
      "Bash(gh label *)",
      "Bash(gh browse *)",
      "Bash(gh search *)",
      "Bash(gh status)",
      "Bash(gh status *)",
      "Bash(gh repo view *)",
      "Bash(gh repo list *)",
      "Bash(gh run list *)",
      "Bash(gh run view *)",
      "Bash(gh run watch *)",
      "Bash(gh workflow list *)",
      "Bash(gh workflow view *)",
      "Bash(gh release list *)",
      "Bash(gh release view *)",
      "Bash(gh release create *)",
      "Bash(gh -C *)",
      "Bash(*bin/flow *)",
      "Bash(bin/test --adversarial-path)",
      "Bash(bin/dependencies)",
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
      "Read(//tmp/*.json)",
      "Agent(flow:adversarial)",
      "Agent(flow:ci-fixer)",
      "Agent(flow:cto)",
      "Agent(flow:documentation)",
      "Agent(flow:issue-triage)",
      "Agent(flow:learn-analyst)",
      "Agent(flow:pm)",
      "Agent(flow:pre-mortem)",
      "Agent(flow:reviewer)",
      "Agent(flow:tech-lead)",
      "Skill(decompose:decompose)"
    ],
    "deny": [
      "Bash(git rebase *)",
      "Bash(git push --force *)",
      "Bash(git push -f *)",
      "Bash(git reset *)",
      "Bash(git reset --hard *)",
      "Bash(git stash *)",
      "Bash(git checkout *)",
      "Bash(git clean *)",
      "Bash(git commit *)",
      "Bash(git config *)",
      "Bash(git branch -d *)",
      "Bash(git branch -D *)",
      "Bash(git symbolic-ref HEAD refs/*)",
      "Bash(git -C * checkout *)",
      "Bash(git -C * clean *)",
      "Bash(git -C * commit *)",
      "Bash(git -C * config *)",
      "Bash(git -C * push --force*)",
      "Bash(git -C * push -f*)",
      "Bash(git -C * rebase *)",
      "Bash(git -C * reset *)",
      "Bash(git -C * stash *)",
      "Bash(sed -i*)",
      "Bash(sed * -i*)",
      "Bash(gh pr merge * --admin*)",
      "Bash(gh pr merge --admin*)",
      "Bash(gh * --admin*)",
      "Bash(gh --admin*)",
      "Bash(gh auth login*)",
      "Bash(gh auth logout*)",
      "Bash(gh auth refresh*)",
      "Bash(gh auth setup-git*)",
      "Bash(gh auth switch*)",
      "Bash(gh auth token*)",
      "Bash(gh extension install *)",
      "Bash(gh issue delete *)",
      "Bash(gh issue lock *)",
      "Bash(gh issue transfer *)",
      "Bash(gh issue unlock *)",
      "Bash(gh label clone *)",
      "Bash(gh label delete *)",
      "Bash(gh release delete *)",
      "Bash(gh repo archive *)",
      "Bash(gh repo delete *)",
      "Bash(gh run cancel *)",
      "Bash(gh run delete *)",
      "Bash(gh secret *)",
      "Bash(gh ssh-key *)",
      "Bash(gh variable *)",
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
      "Bash(python *)",
      "Bash(python3 *)",
      "Bash(python3.10 *)",
      "Bash(python3.11 *)",
      "Bash(python3.12 *)",
      "Bash(python3.13 *)",
      "Bash(pip *)",
      "Bash(pip3 *)",
      "Bash(ruff *)",
      "Bash(pyenv *)",
      "Bash(poetry *)",
      "Bash(uv *)",
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
      "Bash(* | *)",
      "Bash(bash -c *)",
      "Bash(sh -c *)",
      "Bash(zsh -c *)",
      "Bash(eval *)",
      "Bash(xargs *)",
      "Bash(perl -e *)",
      "Bash(perl -E *)",
      "Bash(python -c *)",
      "Bash(python3 -c *)",
      "Bash(ruby -e *)",
      "Bash(node -e *)",
      "Bash(node -p *)",
      "Bash(nc *)",
      "Bash(tmux send-keys *)",
      "Bash(screen -X *)",
      "Bash(ssh *)",
      "Bash(rtk proxy *)"
    ]
  },
  "defaultMode": "acceptEdits"
}
```

### Step 5 — Install plugins

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

### Step 6 — Commit generated files

Check if the working tree has changes by running `git status`. If the output contains "working tree clean", skip to Done.

Otherwise, invoke `/flow:flow-commit` to commit and push the generated files (`.claude/settings.json` and any newly-installed `bin/<tool>` stubs).

### Done — Complete

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.0.1 — Prime — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Report:

- Settings written to `.claude/settings.json`
- Version marker written to `.flow.json` (git-excluded)
- Git excludes configured for `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, `.claude/scheduled_tasks.lock`, `test_adversarial_flow.*`, `adversarial_flow_test.go`, `adversarial_flow_test.rb`, `adversarial_flow_spec.rb`, and `AdversarialFlowTests.swift`
- Pre-commit hook installed — blocks direct `git commit`, requires `/flow:flow-commit`
- Global launcher installed at `~/.local/bin/flow` — run `flow tui` from any primed project
- bin/* stubs installed (list whichever names appear in `stubs_installed` from Step 4); remind the user to edit each one to wire it to their actual toolchain
- Decompose plugin installed (or already present) — DAG planning support from the `matt-k-wong/mkw-DAG-architect` marketplace
- Generated files committed and pushed

Display the skills configuration as a pipe-delimited markdown table with exactly this format (not a bullet list):

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start       | —      | manual   |
| code        | manual | manual   |
| review | auto   | auto     |
| learning    | auto   | auto     |
| complete    | auto   | —        |
| abort       | auto   | —        |
```

Use the actual values from `skills_dict` (Step 1). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.
