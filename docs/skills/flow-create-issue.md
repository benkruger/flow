---
title: /flow-create-issue
nav_order: 16
parent: Skills
---

# /flow-create-issue

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-create-issue <problem description>
/flow:flow-create-issue --step 2
/flow:flow-create-issue --step 3
/flow:flow-create-issue --step 4
```

Decomposes a problem via DAG analysis with deep codebase exploration, iterates with the user until the issue is fully detailed, then files it as a "decomposed" issue ready for autonomous execution via `/flow-start`.

---

## What It Does

Each step is enforced via self-invocation — the skill re-invokes itself with `--step N` after each gate, forcing the model to re-read the full skill instructions at every step boundary.

| Step | Name | Gate |
|------|------|------|
| 1 | Decompose | AskUserQuestion: proceed, iterate, or cancel |
| 2 | Draft | AskUserQuestion: file, revise, or re-decompose |
| 3 | Review | HARD-GATE: approve or iterate |
| 4 | File | Files the issue, shows COMPLETE banner |

1. **Step 1 — Decompose:** Invokes `decompose:decompose` for DAG-based problem breakdown with codebase exploration (Glob, Grep, Read). Presents the synthesis and asks the user to approve, iterate, or cancel.
2. **Step 2 — Draft:** Crafts a comprehensive issue with five sections (Problem, Acceptance Criteria, Files to Investigate, Out of Scope, Context). Presents the full draft inline and asks the user to file, revise, or re-decompose.
3. **Step 3 — Review:** Mandatory approval gate. Presents the draft for final review. User can approve or provide feedback for iteration.
4. **Step 4 — File:** Files the issue via `bin/flow issue` with the `decomposed` label.

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

- Step banners shown at entry to each step (`── Step N of 4: Name ──`)
- AskUserQuestion gates at Steps 1 and 2 — user controls the flow
- Mandatory HARD-GATE approval at Step 3 before filing — no shortcut
- All file paths verified via codebase exploration
- Issues labeled `decomposed` for tracking
- Self-invocation enforcement prevents step skipping
