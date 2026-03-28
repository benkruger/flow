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
/flow:flow-create-issue --step 2 --id <id>
```

Explores a design question or decomposes a concrete problem via DAG analysis with deep codebase exploration, iterates with the user until the issue is fully detailed, then files it as a "decomposed" issue ready for autonomous execution via `/flow-start`.

---

## Input Classification

Before entering the filing pipeline, the skill classifies the user's input:

- **Concrete problem** (bug reports, specific failures, action requests, `#N` issue references) — proceeds directly to the 2-step pipeline below
- **Exploratory question** (design questions like "what could we do with X?", brainstorming, open-ended exploration) — enters Exploration Mode for an interactive design discussion grounded in the codebase
- **Ambiguous** — asks the user which mode they prefer

### Exploration Mode

When the input is exploratory, the skill facilitates a design discussion instead of immediately decomposing. It explores the codebase for relevant context, presents findings and design options, and iterates interactively with the user. Exit paths are wrapped in a HARD-GATE — transitioning to the filing pipeline, ending the exploration, or canceling all require explicit user approval via AskUserQuestion. The user can also end the exploration without filing.

---

## What It Does

Once a concrete problem is identified (either directly or after exploration), Step 1 is enforced via self-invocation — the skill re-invokes itself with `--step 2 --id <id>` after the decompose gate, forcing the model to re-read the full skill instructions at the step boundary. The `<id>` is a short UUID generated in Step 1 that scopes all file paths to prevent concurrent session collisions. A Resume Check reads the step counter from `.flow-states/create-issue-<id>.json` to dispatch correctly on re-entry.

| Step | Name | Gate |
|------|------|------|
| 1 | Decompose | AskUserQuestion: proceed, iterate, or cancel |
| 2 | Draft + File | AskUserQuestion: target project, FLOW plugin, revise, or re-decompose |

1. **Step 1 — Decompose:** Invokes `decompose:decompose` for DAG-based problem breakdown with codebase exploration (Glob, Grep, Read). Presents the synthesis and asks the user to approve, iterate, or cancel. Generates a session ID and writes step counter to `.flow-states/create-issue-<id>.json`.
2. **Step 2 — Draft + File:** Crafts a comprehensive issue with five sections (Problem, Acceptance Criteria, Files to Investigate, Out of Scope, Context). Presents the full draft inline and asks the user where to file in a single AskUserQuestion with four options: file against the target project, file against the FLOW plugin repo (`benkruger/flow`), revise the draft, or re-decompose from scratch. Files the issue via `bin/flow issue`, then cleans up the session-scoped state file.

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

- Step banners shown at entry to each step (`── Step N of 2: Name ──`)
- HARD-GATE on Exploration Mode exit paths — prevents flow breakouts during design discussion
- AskUserQuestion gates at Steps 1 and 2 — user controls the flow
- All AskUserQuestion calls use structured parameters (question, header, options with label+description)
- All file paths verified via codebase exploration
- Issues labeled `decomposed` for tracking (or `Flow` when filed against the plugin repo)
- Repo routing integrated into Step 2 HARD-GATE — user chooses target project or FLOW plugin as part of draft review
- Self-invocation from Step 1 to Step 2 enforces skill re-read at the step boundary
