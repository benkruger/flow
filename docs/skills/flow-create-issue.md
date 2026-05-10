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
/flow:flow-create-issue --force-decompose
```

Captures a brainstormed solution from the current conversation and files it as a pre-planned GitHub issue with an Implementation Plan section. The Plan phase extracts this plan directly — no re-derivation needed. Requires prior brainstorming context (typically via `/decompose:decompose`).

---

## Conversation Gate

Before entering the pipeline, the skill verifies that brainstorming context exists in the conversation — a problem that was explored and a solution that was agreed upon. If no context is found, the skill rejects with guidance to run `/decompose:decompose` first.

---

## What It Does

The skill runs end-to-end as a single pipeline: capture problem sections from the conversation, decompose the implementation (or reuse prior implementation-focused decompose output), transform the synthesis into an Implementation Plan, present the full draft inline, and file the issue against the current repo with the `decomposed` label after explicit user approval.

If prior implementation-focused decompose output already exists in the conversation, the skill skips the decompose invocation. Use `--force-decompose` to bypass the detection and force a fresh decompose.

---

## Issue Format

The filed issue contains enough detail for `/flow-start` to execute fully autonomously, including a pre-built plan that the Plan phase extracts directly:

- **Problem** — grounded in codebase evidence, not theoretical
- **Acceptance Criteria** — binary pass/fail checklist
- **Implementation Plan** — Context, Exploration, Risks, Approach, Dependency Graph, Tasks (matching plan file format)
- **Files to Investigate** — verified paths with relevance notes
- **Context** — business reason and architectural constraints

---

## Gates

- Conversation Gate rejects cold-start invocations without brainstorming context
- AskUserQuestion gate after draft presentation — user controls whether to file, revise, or cancel
- All AskUserQuestion calls use structured parameters (question, header, options with label+description)
- Issues labeled `decomposed` for tracking
- Filing always targets the current repo (no `--repo` flag) — cross-repo filing for FLOW process bugs routes through `flow-learn` Tenant 1 or a manual `bin/flow issue --repo` invocation
