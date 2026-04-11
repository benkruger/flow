---
title: /flow-prime
nav_order: 7
parent: Skills
---

# /flow-prime

**Phase:** Any (run once per install/upgrade)

**Usage:** `/flow-prime` or `/flow-prime --reprime`

One-time project setup. Configures workspace permissions in `.claude/settings.json`, sets up git excludes, installs the `bin/{format,lint,build,test}` delegation stubs, and writes a version marker. Run once after installing FLOW and again after each upgrade.

`--reprime` skips all questions and reuses the existing `.flow.json` config — same autonomy and commit format, just new artifacts installed. Use this for upgrades where your config hasn't changed.

---

## What It Does

1. Asks the user to choose an autonomy level (fully autonomous, fully manual, recommended, or customize per skill)
2. Asks the user to choose a commit message format (title-only or full)
3. Runs a single setup script that handles all configuration in one call:
   - Reads or creates `.claude/settings.json` and merges FLOW universal allow/deny permissions
   - Writes `.flow.json` with version, config hash, commit format, and skills configuration
   - Adds `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, and `.claude/scheduled_tasks.lock` to `.git/info/exclude`
   - Installs a pre-commit hook that blocks direct `git commit` during active FLOW features and requires `/flow:flow-commit`
   - Installs a global launcher at `~/.local/bin/flow`
   - Installs `bin/{format,lint,build,test}` stubs from `assets/bin-stubs/<tool>.sh` into `<project_root>/bin/<tool>` when absent. Pre-existing `bin/*` scripts are never overwritten so users who already configured their own toolchain keep their work.
4. Installs the `decompose` plugin from the `matt-k-wong/mkw-DAG-architect` marketplace
5. Commits generated files (`.claude/settings.json` and any newly-installed `bin/<tool>` stubs) to version control

After prime, the user is responsible for editing each `bin/<tool>` to wire it to their actual toolchain (cargo, pytest, go test, npm, etc.). The default stubs exit 0 with a stderr reminder so a fresh prime never blocks CI.

---

## Repo-Local Tool Delegation

FLOW does not dispatch by language. Every project owns its toolchain inside the four `bin/<tool>` scripts. `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` in sequence (format first for fail-fast). FLOW contributes the orchestration layer (sentinel-based dirty-check, retry/flaky classification, `FLOW_CI_RUNNING` recursion guard, JSON contract) and stays out of the command-string business.

---

## Autonomy Configuration

FLOW has two independent axes for skills that support them:

- **Commit** — controls per-task review in phase skills (auto = skip review prompts, manual = require explicit approval before each commit).
- **Continue** — whether to auto-advance to the next phase or prompt first.

The chosen configuration is stored in `.flow.json` under a `skills` key:

```json
{
  "flow_version": "1.1.0",
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-plan": {"continue": "auto"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-code-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto"
  }
}
```

Phase skills that commit (Code, Code Review, Learn) have both axes as a nested object. Phase skills that don't commit (Start, Plan) have only the continue axis. Utility skills (Abort, Complete) have a single string value.

Phase skills can be overridden at invocation time with `--auto` or `--manual` flags, regardless of the `.flow.json` configuration.

---

## Gates

- Must be in a git repository
- Must be on the main branch (setup runs against main before branching)

---

## See Also

- [/flow-start](flow-start.md) — requires `/flow-prime` to have been run for the current FLOW version
