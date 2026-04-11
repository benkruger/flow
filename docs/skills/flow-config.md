---
title: /flow-config
nav_order: 13
parent: Skills
---

# /flow-config

**Phase:** Any (utility command)

**Usage:** `/flow-config`

Display-only. Reads `.flow.json` from the project root and shows the current FLOW configuration: version and per-skill autonomy settings.

---

## What It Shows

A table of all 7 configurable skills with their autonomy settings across two axes:

- **Commit** — controls per-task review in phase skills (auto = skip review prompts, manual = require explicit approval before each commit).
- **Continue** — whether to auto-advance to the next phase or prompt first.

Phase skills that commit (Code, Code Review, Learn) have both axes. Phase skills that don't commit (Start, Plan) only have Continue. Utility skills (Abort, Complete) have a single mode value shown under Commit.

Phase skills can be overridden at invocation time with `--auto` or `--manual` flags.

---

## See Also

- [/flow-prime](flow-prime.md) — sets up the configuration during project initialization
