---
title: /flow-local-permission
nav_order: 14
parent: Skills
---

# /flow-local-permission

**Phase:** Any (utility skill)

**Usage:** `/flow-local-permission`

Promotes permissions from `.claude/settings.local.json` into
`.claude/settings.json`, then deletes the local file.

---

## When to Use

Use `.claude/settings.local.json` to test new permission entries
during development. When they work, run this skill to promote them
into the committed settings file.

Called automatically by Learning (Phase 5) in Maintainer mode.

---

## What It Does

1. Reads `.claude/settings.local.json`
2. Compares `permissions.allow` entries against `.claude/settings.json`
3. Adds any new entries to `.claude/settings.json`
4. Deletes `.claude/settings.local.json`
5. Reports what was promoted

---

## Rules

- Does nothing if `.claude/settings.local.json` does not exist
- Always promotes all entries (no confirmation prompt)
