---
title: /flow-decompose-project
nav_order: 17
parent: Skills
---

# /flow-decompose-project

**Phase:** Any (standalone)

**Usage:**

```text
/flow:flow-decompose-project <project description>
/flow:flow-decompose-project --step N --id <id>
```

Decomposes a large project into many GitHub issues with native sub-issue relationships, blocked-by dependencies, milestones, and phase labels. Produces a fully linked issue graph ready for autonomous execution via `/flow-start` or `/flow-orchestrate`.

---

## What It Does

Each step is enforced via self-invocation — the skill re-invokes itself with `--step N --id <id>` after each gate, forcing the model to re-read the full skill instructions at every step boundary. The `<id>` is a short UUID generated in Step 1 that scopes all file paths to prevent concurrent session collisions.

| Step | Name | Gate |
|------|------|------|
| 1 | Describe and Decompose | AskUserQuestion: proceed, iterate, or cancel |
| 2 | Review Issue List | AskUserQuestion: due date + approve, revise, or cancel |
| 3 | Create Epic and Milestone | Automatic |
| 4 | Create Child Issues | Automatic |
| 5 | Link Relationships | Automatic |
| 6 | Report | Summary + cleanup |

1. **Step 1 — Describe and Decompose:** Invokes `decompose:decompose` with deep codebase exploration. Presents the DAG synthesis for user review.
2. **Step 2 — Review Issue List:** Builds a complete issue list from the DAG with titles, bodies, phase labels, and dependencies. Asks for milestone due date. User approves the full list before any issues are created.
3. **Step 3 — Create Epic and Milestone:** Creates the milestone with the due date and the parent epic issue.
4. **Step 4 — Create Child Issues:** Creates all child issues in topological order (leaves first) so dependency numbers exist when referenced. Each issue gets the "Decomposed" label and an auto-derived phase label.
5. **Step 5 — Link Relationships:** Sets sub-issue relationships (children to epic) and blocked-by dependencies (between children per DAG) via GitHub REST API. Best-effort throughout.
6. **Step 6 — Report:** Presents a summary table of everything created, then cleans up session files.

---

## Issue Format

Each child issue contains:

- **Problem** — grounded in codebase evidence
- **Acceptance Criteria** — binary pass/fail checklist
- **Files to Investigate** — verified paths
- **Out of Scope** — explicit boundaries
- **Context** — business reason and constraints
- **Depends on: #N** — text-based dependency references (belt and suspenders with API links)

---

## GitHub Relationships

The skill creates three types of GitHub relationships:

- **Sub-issues** — each child issue is linked as a sub-issue of the epic
- **Blocked-by** — dependency relationships between child issues per the DAG
- **Milestone** — all issues assigned to a milestone with the user-specified due date

All relationship creation is best-effort. Text-based "Depends on: #N" references in issue bodies provide a fallback if API linking fails.

---

## Gates

- HARD-GATE on Step 1 — user approves decomposition before drafting issues
- HARD-GATE on Step 2 — user approves complete issue list and due date before creation
- No issues created until both gates pass
- Self-invocation enforcement prevents step skipping
- Session state in `.flow-states/decompose-project-<id>.json` enables resume
