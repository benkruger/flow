---
title: "Phase 6: Security"
nav_order: 7
---

# Phase 6: Security

**Command:** `/flow:security`

Security analysis of the feature diff. Scans for vulnerabilities,
authentication gaps, data exposure, and injection risks. Review
confirmed that the code matches the plan — Security confirms
the code is safe.

---

## What Security Checks

The security sub-agent runs 10 checks against the full diff. The specific checks are defined by the framework instructions in the skill — each framework has its own security checklist tailored to its common vulnerability patterns (e.g., authorization gaps and CSRF for Rails; command injection and path traversal for Python).

---

## Findings

Every confirmed finding gets fixed. No severity tiers — fix one finding,
commit, then move to the next.

---

## bin/flow ci Rule

`bin/flow ci` runs after every fix made during Security.
Security does not transition to Learning until `bin/flow ci` is green.

---

## What Comes Next

Phase 7: Learning (`/flow:learning`) — extract learnings and update
CLAUDE.md before the PR is merged.
