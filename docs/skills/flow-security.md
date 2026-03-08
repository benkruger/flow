---
title: /flow-security
nav_order: 9
parent: Skills
---

# /flow-security

**Phase:** 6 — Security

**Usage:** `/flow-security`, `/flow-security --auto`, or `/flow-security --manual`

Delegates to Claude's built-in `/security-review` command for
language-aware security analysis of the branch diff. Fixes every
finding, runs `bin/flow ci` after every fix.

---

## Fixing Findings

Every confirmed finding gets fixed directly:

1. Fix one finding
2. Run `bin/flow ci`
3. Commit via `/flow-commit`
4. Mark finding as fixed in state
5. Next finding

---

## Mode

Both commit and continue are configurable via `.flow.json` (defaults: both auto). Commit mode controls whether security fix commits require diff approval. Continue mode controls whether the phase transition advances to Learning automatically or prompts first.

---

## Gates

- Phase 5: Review must be complete
- `bin/flow ci` must be green after every fix
- `bin/flow ci` must be green before transitioning to Learning
- Full diff must be read before analysis begins
