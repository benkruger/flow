---
title: /flow-prime
nav_order: 7
parent: Skills
---

# /flow-prime

**Phase:** Any (run once per install/upgrade)

**Usage:** `/flow-prime` or `/flow-prime --reprime`

One-time project setup. Configures workspace permissions in `.claude/settings.json`, sets up git excludes, and writes a version marker. Run once after installing FLOW and again after each upgrade.

`--reprime` skips all questions and reuses the existing `.flow.json` config — same framework, autonomy, and commit format, just new artifacts installed. Use this for upgrades where your config hasn't changed.

---

## What It Does

1. Auto-detects framework using data-driven detection (`frameworks/*/detect.json`) and checks plugin availability in parallel
2. Asks the user to choose an autonomy level (fully autonomous, fully manual, recommended, or customize per skill)
3. Asks the user to choose a commit message format (title-only or full)
4. Runs a single setup script that handles all configuration in one call:
   - Reads or creates `.claude/settings.json` and merges FLOW allow/deny permissions (universal + framework-specific + derived)
   - Writes `.flow.json` with version, framework, config hash, commit format, and skills configuration
   - Adds `.flow-states/`, `.worktrees/`, `.flow.json`, `bin/dependencies`, and `.claude/scheduled_tasks.lock` to `.git/info/exclude`
   - Installs a pre-commit hook that blocks direct `git commit` during active FLOW features and requires `/flow:flow-commit`
   - Primes the project CLAUDE.md with framework conventions from `frameworks/<name>/priming.md`
   - Creates `bin/dependencies` from the framework template
5. Installs the `decompose` plugin from the `matt-k-wong/mkw-DAG-architect` marketplace
6. Asks whether to commit generated files (`CLAUDE.md`, `.claude/settings.json`) or git-exclude them (keep local-only)

---

## Autonomy Configuration

FLOW has two independent axes for skills that support them:

- **Commit** — how `/flow-commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code.
- **Continue** — whether to auto-advance to the next phase or prompt first.

The chosen configuration is stored in `.flow.json` under a `skills` key:

```json
{
  "flow_version": "0.16.4",
  "framework": "python",
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

Phase skills that commit (Code, Code Review, Learn) have both axes as a nested object. Phase skills that don't commit (Start, Plan) have only the continue axis. Utility skills (Abort, Complete) have a single string value. The `/flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Individual skills can always be overridden at invocation time with `--auto` or `--manual` flags, regardless of the `.flow.json` configuration.

---

## Derived Permissions

Some permissions are project-specific but can be automatically determined from project files. Frameworks can declare `derived_permissions` in `frameworks/<name>/permissions.json` with glob patterns and templates. For example, iOS derives `Bash(killall AppName)` from the `*.xcodeproj` directory name — no user input needed.

Derived permissions are merged into `.claude/settings.json` automatically during setup alongside universal and framework permissions.

---

## Gates

- Must be in a git repository
- Must be on the main branch (setup runs against main before branching)

---

## See Also

- [/flow-start](flow-start.md) — requires `/flow-prime` to have been run for the current FLOW version
