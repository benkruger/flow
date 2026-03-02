---
title: "Phase 5: Security"
nav_order: 6
---

# Phase 5: Security

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

## bin/ci Rule

bin/ci runs after every fix made during Security.
Security does not transition to Reflect until bin/ci is green.

---

## What Comes Next

Phase 6: Reflect (`/flow:reflect`) — extract learnings and update
CLAUDE.md before the PR is merged.
