---
title: /flow:init
nav_order: 7
parent: Skills
---

# /flow:init

**Phase:** Any (run once per install/upgrade)

**Usage:** `/flow:init`

One-time project setup. Configures workspace permissions in `.claude/settings.json`, sets up git excludes, and writes a version marker. Run once after installing FLOW and again after each upgrade.

---

## What It Does

1. Auto-detects framework from project files (Gemfile → Rails, pyproject.toml/setup.py/requirements.txt → Python) and confirms with the user
2. Asks the user to choose an autonomy level (fully autonomous, fully manual, recommended, or customize per skill)
3. Reads `.claude/settings.json` (or starts with `{}`)
4. Merges FLOW allow/deny permission entries (universal + framework-specific), preserving existing entries
5. Writes the merged `.claude/settings.json`
6. Writes `.flow.json` with the current FLOW version, framework, and skills configuration
7. Adds `.flow-states/` and `.worktrees/` to `.git/info/exclude`
8. Commits `.claude/settings.json` and `.flow.json`

---

## Autonomy Configuration

FLOW has two independent axes for skills that support them:

- **Commit** — how `/flow:commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code and refactoring approval in Simplify.
- **Continue** — whether to auto-advance to the next phase or prompt first.

The chosen configuration is stored in `.flow.json` under a `skills` key:

```json
{
  "flow_version": "0.16.4",
  "framework": "python",
  "skills": {
    "start": {"continue": "manual"},
    "code": {"commit": "manual", "continue": "manual"},
    "simplify": {"commit": "auto", "continue": "auto"},
    "review": {"commit": "auto", "continue": "auto"},
    "security": {"continue": "auto"},
    "learning": {"commit": "auto", "continue": "auto"},
    "abort": "auto",
    "cleanup": "auto"
  }
}
```

Phase skills that commit (Code, Simplify, Review, Learning) have both axes as a nested object. Phase skills that don't commit (Start, Security) have only the continue axis. Utility skills (Abort, Cleanup) have a single string value. The `/flow:commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Individual skills can always be overridden at invocation time with `--auto` or `--manual` flags, regardless of the `.flow.json` configuration.

---

## Gates

- Must be in a git repository
- Must be on the main branch (permissions are committed and shared with the team)

---

## See Also

- [/flow:start](flow-start.md) — requires `/flow:init` to have been run for the current FLOW version
