---
title: Skills
nav_order: 3
---

# Skills

Skills are the building blocks of the FLOW workflow. Some are tied to a specific phase and invoked automatically as part of that phase. Others are utility skills available at any point.

All skills announce themselves clearly when they start and finish.

---

## Phase Skills

These skills correspond directly to a workflow phase. Each one starts and ends with a banner so you always know where you are.

| Skill | Phase | Description |
|-------|-------|-------------|
| [`/flow-start`](flow-start.md) | 1 — Start | Create the worktree, upgrade dependencies, open the PR |
| [`/flow-code`](flow-code.md) | 2 — Code | TDD task by task, diff review, `bin/flow ci` gate, plan test verification before each commit |
| [`/flow-review`](flow-review.md) | 3 — Review | Six tenants assessed by four agents (reviewer, pre-mortem, adversarial, documentation) — gather, launch, triage, fix |
| [`/flow-learn`](flow-learn.md) | 4 — Learn | Extract learnings, update CLAUDE.md, note plugin gaps |
| [`/flow-complete`](flow-complete.md) | 5 — Complete | Merge PR, remove worktree, delete state file — final phase |

---

## Utility Skills

These skills are available at any point in the workflow, regardless of phase.

| Skill | Description |
|-------|-------------|
| [`/flow-prime`](flow-prime.md) | One-time setup — configure permissions, capture primary role, install bin/* delegation stubs, and configure git excludes |
| [`/flow-commit`](flow-commit.md) | Review the full diff, then git add + commit + push |
| [`/flow-note`](flow-note.md) | Capture a correction or learning — invoked automatically on corrections |
| [`/flow-abort`](flow-abort.md) | Abandon the current feature — close PR, delete branch, remove worktree |
| [`/flow-continue`](flow-continue.md) | Resume a halted autonomous flow — clears `_halt_pending` so the next assistant turn proceeds |
| [`/flow-reset`](flow-reset.md) | Wipe `.flow-states/` on this machine after explicit confirmation. PRs, worktrees, and branches are NOT touched (those require per-flow `/flow-abort`) |
| [`/flow-config`](flow-config.md) | Display current configuration — version and per-skill autonomy |
| [`/flow-skills`](flow-skills.md) | Display the FLOW skill catalog grouped by user role — Maintainer and Private buckets render only inside the FLOW plugin repo |
| [`/flow-doc-sync`](flow-doc-sync.md) | Full codebase documentation accuracy review — reports drift between code and docs |
| [`/flow-hygiene`](flow-hygiene.md) | Audit instruction corpus health — CLAUDE.md, rules, and memory for staleness, misplacement, duplication, and contradictions |
| [`/flow-issues`](flow-issues.md) | Group open issues by label into four sections (Blocked, Other, Vanilla, Decomposed) with mechanical sort and a copy-pasteable command per row. Supports section filters (`--ready`, `--blocked`, `--decomposed`, `--quick-start`) and narrowing filters (`--label`, `--milestone`) |
| [`/flow-explore`](flow-explore.md) | Open a problem-statement conversation (PM voice) — discussion-mode by default, files a vanilla `## What` / `## Why` / `## Acceptance Criteria` issue with `--label vanilla` on user signal |
| [`/flow-plan`](flow-plan.md) | Produce a structured implementation plan and attach it to a GitHub issue. Accepts `#N` (re-plans the existing issue in place, preserving content above the FLOW-PLAN sentinel) or a bare prompt (synthesizes `## What` / `## Why` / `## Acceptance Criteria` and files a new decomposed issue). Tech Lead voice, mandatory `decompose:decompose` pass, attaches the `decomposed` label in both modes |
| [`/flow-decompose-project`](flow-decompose-project.md) | Decompose a large project into linked GitHub issues with sub-issue relationships, blocked-by dependencies, and phase labels |
| [`/flow-orchestrate`](flow-orchestrate.md) | Process decomposed issues sequentially overnight via flow-start --auto |
| [`/flow-triage-issue`](flow-triage-issue.md) | Triage a single open GitHub issue from a PM lens. Reads code, checks for already-shipped work, returns a verdict in `{close, decompose}` |
