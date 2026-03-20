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

Explores a design question or decomposes a concrete problem via DAG analysis with deep codebase exploration, iterates with the user until the issue is fully detailed, then files it as a "decomposed" issue ready for autonomous execution via `/flow-start`.

---

## Input Classification

Before entering the filing pipeline, the skill classifies the user's input:

- **Concrete problem** (bug reports, specific failures, action requests, `#N` issue references) — proceeds directly to the 4-step pipeline below
- **Exploratory question** (design questions like "what could we do with X?", brainstorming, open-ended exploration) — enters Exploration Mode for an interactive design discussion grounded in the codebase
- **Ambiguous** — asks the user which mode they prefer

### Exploration Mode

When the input is exploratory, the skill facilitates a design discussion instead of immediately decomposing. It explores the codebase for relevant context, presents findings and design options, and iterates interactively with the user. When the discussion produces a concrete problem to file, the skill transitions into the standard 4-step pipeline. The user can also end the exploration without filing.

---

## What It Does

Once a concrete problem is identified (either directly or after exploration), each step is enforced via self-invocation — the skill re-invokes itself with `--step N` after each gate, forcing the model to re-read the full skill instructions at every step boundary.

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
