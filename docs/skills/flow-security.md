---
title: /flow:security
nav_order: 9
parent: Skills
---

# /flow:security

**Phase:** 7 — Security

**Usage:** `/flow:security`

Security analysis of the feature diff. Uses an Explore sub-agent
to run 10 security checks, then fixes every confirmed finding
one at a time with a commit per fix.

---

## What It Checks

The 10 security checks are defined by the framework fragment. Each framework has its own checklist tailored to common vulnerability patterns (e.g., authorization gaps and CSRF bypass for Rails; command injection and path traversal for Python).

---

## Fixing Findings

Every confirmed finding gets fixed directly:

1. Fix one finding
2. Run `bin/ci`
3. Commit via `/flow:commit`
4. Mark finding as fixed in state
5. Next finding

---

## Gates

- Phase 6: Review must be complete
- bin/ci must be green after every fix
- bin/ci must be green before transitioning to Reflect
- Full diff must be read before analysis begins
