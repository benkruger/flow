---
title: /flow-config
nav_order: 13
parent: Skills
---

# /flow-config

**Phase:** Any (utility command)

**Usage:** `/flow-config`

Display-only. Reads `.flow.json` from the project root and shows the current FLOW configuration: version, framework, and per-skill autonomy settings.

---

## What It Shows

A table of all 8 configurable skills with their autonomy settings across two axes:

- **Commit** — how `/flow-commit` is invoked during phase work (auto = skip diff approval, manual = require approval). Also controls per-task approval in Code and refactoring approval in Simplify.
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (Code, Simplify, Review, Security, Learning) have both axes. Phase skills that don't commit (Start) only has Continue. Utility skills (Abort, Cleanup) have a single mode value shown under Commit. The `/flow-commit` skill is not configurable — it defaults to auto and can be overridden with `--manual`.

Any setting can be overridden at invocation time with `--auto` or `--manual` flags.

---

## See Also

- [/flow-prime](flow-prime.md) — sets up the configuration during project initialization
