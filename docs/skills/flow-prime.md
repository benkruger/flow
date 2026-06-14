---
title: /flow-prime
nav_order: 7
parent: Skills
---

# /flow-prime

**Phase:** Any (run once per install/upgrade)

**Usage:** `/flow-prime` or `/flow-prime --reprime`

One-time project setup. Configures workspace permissions in `.claude/settings.json`, sets up git excludes, installs the `bin/{format,lint,build,test}` delegation stubs, and writes a version marker. Run once after installing FLOW and again after each upgrade.

`--reprime` skips all questions and reuses the existing `.flow.json` config — same autonomy, just new artifacts installed. Use this for upgrades where your config hasn't changed.

---

## What It Does

1. Asks the user for their primary role — PM, Tech Lead (recommended), or Founder / Solo Dev. The selection is recorded as the `role` field in `.flow.json` and sets a default planning persona for future planning conversations.
2. Asks the user to choose an autonomy level (fully autonomous, fully manual, recommended, or customize per skill).
3. Runs a single setup script that handles all configuration in one call:
   - Reads or creates `.claude/settings.json` and merges FLOW universal allow/deny permissions
   - Writes `.flow.json` with version, config hash, role (when set), and skills configuration
   - Adds `.flow-states/`, `.worktrees/`, `.flow.json`, `.claude/cost/`, `.claude/scheduled_tasks.lock`, `test_adversarial_flow.*`, `adversarial_flow_test.go`, `adversarial_flow_test.rb`, `adversarial_flow_spec.rb`, and `AdversarialFlowTests.swift` to `.git/info/exclude`
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
  "flow_version": "2.5.0",
  "skills": {
    "flow-start": {"continue": "auto"},
    "flow-code": {"commit": "auto", "continue": "auto"},
    "flow-review": {"commit": "auto", "continue": "auto"},
    "flow-abort": {"continue": "manual"},
    "flow-complete": {"continue": "manual"}
  }
}
```

Every skill's config is an object. Phase skills that commit (Code, Review) carry both axes — `{"commit": ..., "continue": ...}`. Phase skills that don't commit (Start) and the utility skills (Abort, Complete) carry only the continue axis — `{"continue": ...}`.

`.flow.json` is the single source of truth for skill autonomy: each skill resolves its mode from its `skills.<skill>` config via `resolve-skill-mode`. There are no `--auto`/`--manual` invocation flags.

---

## Gates

- Must be in a git repository
- Must be on the integration branch (`main`, `staging`, or whatever the repo's default branch is) — setup runs against the integration branch before branching

---

## See Also

- [/flow-start](flow-start.md) — requires `/flow-prime` to have been run for the current FLOW version
