---
title: /flow-doc-sync
nav_order: 14
parent: Skills
---

# /flow-doc-sync

**Phase:** Any

**Usage:**

```text
/flow-doc-sync
```

Full codebase documentation accuracy review. Compares behavioral sources (skills, lib scripts, config files) against all documentation surfaces (README, docs pages, CLAUDE.md, rules) and produces a severity-tagged drift report. Read-only — reports drift but does not fix anything.

---

## What It Does

1. Discovers project structure by reading CLAUDE.md and using Glob to find all documentation surfaces (README.md, docs/\*\*/\*.md, docs/\*\*/\*.html, .claude/rules/\*.md)
2. Reads all behavioral sources identified from CLAUDE.md and all documentation surfaces
3. Cross-references each doc surface against behavioral sources, tagging findings by severity
4. Produces an inline drift report with a summary line and per-file findings

---

## Severity Tags

| Tag | Meaning |
|-----|---------|
| `[STALE]` | Doc describes behavior that has changed — feature exists but works differently |
| `[MISSING]` | Behavior exists in code but is not documented in any surface |
| `[OUTDATED]` | Doc references something that no longer exists — removed file, renamed command |

Each finding includes doc-says/code-does pairs with source file references.

---

## Gates

- Read-only — never fixes, edits, or commits anything
- No state file mutations — stateless utility skill
- Display-only — no AskUserQuestion prompts
