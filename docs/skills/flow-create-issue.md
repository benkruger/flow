---
title: /flow-create-issue
nav_order: 16
parent: Skills
---

# /flow-create-issue

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-create-issue
/flow:flow-create-issue --step 2 --id <id>
```

Captures a brainstormed solution from the current conversation and files it as a pre-planned GitHub issue with an Implementation Plan section. The Plan phase extracts this plan directly — no re-derivation needed. Requires prior brainstorming context (typically via `/decompose:decompose`).

---

## Conversation Gate

Before entering the pipeline, the skill verifies that brainstorming context exists in the conversation — a problem that was explored and a solution that was agreed upon. If no context is found, the skill rejects with guidance to run `/decompose:decompose` first.

---

## What It Does

Step 1 is enforced via self-invocation — the skill re-invokes itself with `--step 2 --id <id>` after the decompose gate, forcing the model to re-read the full skill instructions at the step boundary. The `<id>` is a short UUID generated in Step 1 that scopes all file paths to prevent concurrent session collisions. A Resume Check reads the step counter from `.flow-states/create-issue-<id>.json` to dispatch correctly on re-entry.

| Step | Name | Gate |
|------|------|------|
| 1 | Capture + Decompose Implementation | AskUserQuestion: proceed, iterate, or cancel |
| 2 | Transform + Draft + File | AskUserQuestion: file (3 options in FLOW repo, 4 otherwise), revise, or re-decompose |

1. **Step 1 — Capture + Decompose Implementation:** Captures problem sections (Problem, Acceptance Criteria, Files to Investigate, Out of Scope, Context) from the conversation context. Then invokes `decompose:decompose` with an implementation-focused prompt — structuring the agreed solution into tasks with dependencies, not re-analyzing the problem. Presents the synthesis and asks the user to approve, iterate, or cancel.
2. **Step 2 — Transform + Draft + File:** Transforms the decompose output into an Implementation Plan section (Context, Exploration, Risks, Approach, Dependency Graph, Tasks) matching the plan file format that `flow-plan` reads. Combines with the captured problem sections into a single issue body. Presents the full draft inline and asks the user where to file. When the current repo is `benkruger/flow`, the skill detects this via `git remote get-url origin` and presents a simplified 3-option prompt. Files the issue via `bin/flow issue` with the `decomposed` label.

---

## Issue Format

The filed issue contains enough detail for `/flow-start` to execute fully autonomously, including a pre-built plan that the Plan phase extracts directly:

- **Problem** — grounded in codebase evidence, not theoretical
- **Acceptance Criteria** — binary pass/fail checklist
- **Implementation Plan** — Context, Exploration, Risks, Approach, Dependency Graph, Tasks (matching plan file format)
- **Files to Investigate** — verified paths with relevance notes
- **Out of Scope** — explicit boundaries to prevent scope creep
- **Context** — business reason and architectural constraints

---

## Gates

- Step banners shown at entry to each step (`── Step N of 2: Name ──`)
- Conversation Gate rejects cold-start invocations without brainstorming context
- AskUserQuestion gates at Steps 1 and 2 — user controls the flow
- All AskUserQuestion calls use structured parameters (question, header, options with label+description)
- Issues labeled `decomposed` for tracking (or `Flow` when filed against the plugin repo)
- Repo routing integrated into Step 2 HARD-GATE — when in the FLOW repo (`benkruger/flow`), repo selection is skipped; otherwise user chooses target project or FLOW plugin
- Self-invocation from Step 1 to Step 2 enforces skill re-read at the step boundary
