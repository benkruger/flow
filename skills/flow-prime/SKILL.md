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
  FLOW v2.5.0 — Prime — STARTING
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

### Step 1 — Choose primary role

The user's primary role sets a default planning persona for future
planning conversations. PM users get the Tech Lead voice by default,
Tech Lead users get the PM voice, and founder/solo users wear
multiple hats — `/flow:flow-explore` opens with the PM voice and
`/flow:flow-plan` opens with the Tech Lead voice, with the ability
to invite the other voice mid-conversation.

<HARD-GATE>
You MUST ask the user with AskUserQuestion below and wait for an
explicit selection before proceeding. Do NOT infer the answer from
context, do NOT default to any option, and do NOT skip the prompt
even when reprime appears to already carry a role. The role choice
is a user decision per `.claude/rules/skill-authoring.md` "Decision
Point Gates".

> "What is your primary role? Sets a default planning persona."
>
> - **Tech Lead (Recommended)** — Engineering lead role
> - **PM** — Product / project management role
> - **Founder / Solo Dev** — Wear multiple hats

</HARD-GATE>

Store the result as `role_value`:

- "Tech Lead" → `"tech-lead"`
- "PM" → `"pm"`
- "Founder / Solo Dev" → `"founder-solo"`

### Step 2 — Choose commit message format

FLOW supports two commit message formats:

- **Full** — subject + tl;dr + explanation + file list (detailed seven-element format)
- **Title only** — subject line + file list (minimal, no tl;dr section)

Ask the user which format to use with AskUserQuestion:

> "What commit message format should FLOW use?"
>
> - **Full format (Recommended)** — "Subject + tl;dr + explanation + file list (detailed)"
> - **Title only** — "Subject line + file list, no tl;dr section"

Store the result as `commit_format`:

- "Full format" → `"full"`
- "Title only" → `"title-only"`

### Step 3 — Choose autonomy level

FLOW has two independent axes for skills that support them:

- **Commit** — controls per-task review in phase skills (auto = skip review prompts, manual = require explicit approval before each commit).
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (code, review, learning) have both axes. Phase skills that don't commit (start) only have continue. Utility skills (complete, abort) have only the continue axis. Every skill's config is an object — `{"commit": ..., "continue": ...}` for the committing phases, `{"continue": ...}` for start, complete, and abort.

Ask the user how much autonomy FLOW should have using AskUserQuestion:

> "How much autonomy should FLOW have?"
>
> - **Fully autonomous** — "All skills auto for both commit and continue"
> - **Fully manual** — "All skills manual for both commit and continue"
> - **Recommended** — "Auto where safe, manual where judgment matters (default)"
> - **Customize** — "Choose per skill and axis"

**Fully autonomous** — all auto:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": {"continue": "auto"}, "flow-abort": {"continue": "auto"}}
```

**Fully manual** — all manual:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "manual", "continue": "manual"}, "flow-review": {"commit": "manual", "continue": "manual"}, "flow-learn": {"commit": "manual", "continue": "manual"}, "flow-complete": {"continue": "manual"}, "flow-abort": {"continue": "manual"}}
```

**Recommended** — safe defaults:

```json
{"flow-start": {"continue": "auto"}, "flow-code": {"commit": "auto", "continue": "auto"}, "flow-review": {"commit": "auto", "continue": "auto"}, "flow-learn": {"commit": "auto", "continue": "auto"}, "flow-complete": {"continue": "manual"}, "flow-abort": {"continue": "manual"}}
```

**Customize** — ask per skill, in this order: code, review, learn, complete, abort.

Start is exempt from the Customize loop because every preset fixes its continue mode to `auto` — Start has no useful interaction to gate on, so prompting for it would only add friction. Before asking the per-skill questions below, **seed `skills_dict` with `{"flow-start": {"continue": "auto"}}`** so the resulting JSON carries Start's continue mode through to Step 4.

For each remaining skill, ask about only the applicable axes. List the recommended option first with "(Recommended)" in the label:

For **code** (commit and continue), ask two AskUserQuestions:

First question:

> "Commit mode for /flow:flow-code? (controls per-task review before each commit)"
>
> - **Auto (Recommended)** — "Skip approval prompts"
> - **Manual** — "Require explicit approval"

Second question:

> "Continue mode for /flow:flow-code? (controls phase advancement)"
>
> - **Auto (Recommended)** — "Auto-advance to next phase"
> - **Manual** — "Prompt before advancing"

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

For **complete** and **abort** (continue axis only), ask one
AskUserQuestion each:

> "Continue mode for /flow:flow-<skill>?"
>
> - **Manual (Recommended)** — "Require confirmation prompt"
> - **Auto** — "Skip confirmation prompt"

Store each answer as `{"continue": "<mode>"}` — the same object shape
the presets use — so `flow-complete` and `flow-abort` carry a
`continue` axis in `skills_dict`.

Store the result as `skills_dict` for Step 4.

### Step 4 — Run prime setup script

Serialize `skills_dict` from Step 3 as a JSON string for the `--skills-json` argument.
Pass the `commit_format` value from Step 2 via `--commit-format`.
Pass the concrete `role_value` from Step 1 via `--role`.

**Do not pass the literal string `<role_value>` as the flag argument** —
the role-selection step yielded a concrete value (pm, tech-lead,
founder-solo) that goes after `--role`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow prime-setup <project_root> --skills-json '<skills_dict_json>' --commit-format <commit_format> --role <role_value> --plugin-root ${CLAUDE_PLUGIN_ROOT}
```

When the Reprime path carries forward a legacy `.flow.json` that has
no `role` field (written before role selection existed), omit
`--role` entirely:

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
      "Read(//tmp/*.jsonl)",
      "Write(//tmp/*.txt)",
      "Write(//tmp/*.diff)",
      "Write(//tmp/*.patch)",
      "Write(//tmp/*.md)",
      "Write(//tmp/*.json)",
      "Write(//tmp/*.jsonl)",
      "Agent(flow:adversarial)",
      "Agent(flow:ci-fixer)",
      "Agent(flow:cto)",
      "Agent(flow:documentation)",
      "Agent(flow:learn-analyst)",
      "Agent(flow:pm)",
      "Agent(flow:pre-mortem)",
      "Agent(flow:reviewer)",
      "Agent(flow:tech-lead)",
      "Skill(decompose:decompose)",
      "Skill(flow:flow-code)",
      "Skill(flow:flow-commit)",
      "Skill(flow:flow-complete)",
      "Skill(flow:flow-config)",
      "Skill(flow:flow-doc-sync)",
      "Skill(flow:flow-explore)",
      "Skill(flow:flow-hygiene)",
      "Skill(flow:flow-issues)",
      "Skill(flow:flow-learn)",
      "Skill(flow:flow-note)",
      "Skill(flow:flow-orchestrate)",
      "Skill(flow:flow-plan)",
      "Skill(flow:flow-review)",
      "Skill(flow:flow-skills)",
      "Skill(flow:flow-start)",
      "Skill(flow:flow-triage-issue)"
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
  ✓ FLOW v2.5.0 — Prime — COMPLETE
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
| start       | —      | auto     |
| code        | auto   | auto     |
| review      | auto   | auto     |
| learning    | auto   | auto     |
| complete    | manual | —        |
| abort       | manual | —        |
```

Use the actual values from `skills_dict` (Step 3). The table above is just an example. Show `—` for axes that don't apply to a skill. The table must use pipe `|` delimiters — never render as a bullet list.
