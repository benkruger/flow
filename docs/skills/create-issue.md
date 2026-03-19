---
title: /create-issue
nav_order: 16
parent: Skills
---

# /create-issue

**Phase:** Any (standalone)

**Usage:** `/flow:create-issue <problem description>`

Decomposes a problem via DAG analysis with deep codebase exploration, iterates with the user until the issue is fully detailed, then files it as a "decomposed" issue ready for autonomous execution via `/flow-start`.

---

## What It Does

1. Invokes the `decompose:decompose` plugin for DAG-based problem breakdown with codebase exploration (Glob, Grep, Read)
2. Drafts a comprehensive issue with five sections: Problem, Acceptance Criteria, Files to Investigate, Out of Scope, and Context
3. Presents the full draft inline for user review — mandatory approval gate
4. Iterates on feedback until the user approves
5. Files the issue via `bin/flow issue` with the `decomposed` label

---

## Issue Format

The filed issue contains enough detail for `/flow-start` to execute fully autonomously:

- **Problem** — grounded in codebase evidence, not theoretical
- **Acceptance Criteria** — binary pass/fail checklist
- **Files to Investigate** — verified paths with relevance notes
- **Out of Scope** — explicit boundaries to prevent scope creep
- **Context** — business reason and architectural constraints

---

## Gates

- Mandatory user approval before filing — no shortcut
- All file paths verified via codebase exploration
- Issues labeled `decomposed` for tracking
