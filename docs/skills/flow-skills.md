---
title: /flow-skills
nav_order: 20
parent: Skills
---

# /flow-skills

**Phase:** Any (utility command)

**Usage:** `/flow:flow-skills`

Display-only. Reports the FLOW skill catalog grouped by user role. Reads no state.

---

## What It Shows

Four sections render in every project:

- **Planning** — issues, triage, create, decompose, orchestrate.
- **Work** — start, config, skills.
- **Health** — doc-sync, hygiene.
- **Admin** — prime, abort, reset (user-only — type the slash command directly).

Two additional sections render only when the current repo is the FLOW plugin source (detected via `git remote get-url origin` matching `benkruger/flow`):

- **Maintainer** — qa, release.
- **Private** — phase skills (code, review, learn, complete) and helpers (commit, status, note) that other FLOW skills invoke. The user does not type these directly.

---

## See Also

- [/flow-config](flow-config.md) — display the per-skill autonomy configuration.
