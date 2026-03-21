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
/flow:flow-create-issue --step 3 --id <id>
```

Explores a design question or decomposes a concrete problem via DAG analysis with deep codebase exploration, iterates with the user until the issue is fully detailed, then files it as a "decomposed" issue ready for autonomous execution via `/flow-start`.

---

## Input Classification

Before entering the filing pipeline, the skill classifies the user's input:

- **Concrete problem** (bug reports, specific failures, action requests, `#N` issue references) — proceeds directly to the 3-step pipeline below
- **Exploratory question** (design questions like "what could we do with X?", brainstorming, open-ended exploration) — enters Exploration Mode for an interactive design discussion grounded in the codebase
- **Ambiguous** — asks the user which mode they prefer

### Exploration Mode

When the input is exploratory, the skill facilitates a design discussion instead of immediately decomposing. It explores the codebase for relevant context, presents findings and design options, and iterates interactively with the user. Exit paths are wrapped in a HARD-GATE — transitioning to the filing pipeline, ending the exploration, or canceling all require explicit user approval via AskUserQuestion. The user can also end the exploration without filing.

---

## What It Does

Once a concrete problem is identified (either directly or after exploration), each step is enforced via self-invocation — the skill re-invokes itself with `--step N --id <id>` after each gate, forcing the model to re-read the full skill instructions at every step boundary. The `<id>` is a short UUID generated in Step 1 that scopes all file paths to prevent concurrent session collisions. A Resume Check reads the step counter from `.flow-states/create-issue-<id>.json` to dispatch correctly on re-entry.

| Step | Name | Gate |
|------|------|------|
| 1 | Decompose | AskUserQuestion: proceed, iterate, or cancel |
| 2 | Draft + Review | AskUserQuestion: file, revise, or re-decompose (iteration loop) |
| 3 | File | Files the issue, shows COMPLETE banner |

1. **Step 1 — Decompose:** Invokes `decompose:decompose` for DAG-based problem breakdown with codebase exploration (Glob, Grep, Read). Presents the synthesis and asks the user to approve, iterate, or cancel. Generates a session ID and writes step counter to `.flow-states/create-issue-<id>.json`.
2. **Step 2 — Draft + Review:** Crafts a comprehensive issue with five sections (Problem, Acceptance Criteria, Files to Investigate, Out of Scope, Context). Presents the full draft inline with an iteration loop — user can revise as many times as needed before approving. On approval, persists the draft to `.flow-states/create-issue-<id>-draft.md`.
3. **Step 3 — File:** Asks the user whether to file against the target project or the FLOW plugin repo (`benkruger/flow`). Reads the approved draft from disk, files the issue via `bin/flow issue` with the appropriate label and repo flag, then cleans up the session-scoped state and draft files.

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

- Step banners shown at entry to each step (`── Step N of 3: Name ──`)
- HARD-GATE on Exploration Mode exit paths — prevents flow breakouts during design discussion
- AskUserQuestion gates at Steps 1 and 2 — user controls the flow
- Draft persisted to disk after approval in Step 2 — survives context loss
- All file paths verified via codebase exploration
- Issues labeled `decomposed` for tracking (or `Flow` when filed against the plugin repo)
- Repo routing HARD-GATE in Step 3 — user chooses target project or FLOW plugin
- Self-invocation enforcement prevents step skipping
