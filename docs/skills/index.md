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
| [`/flow-reset`](flow-reset.md) | Remove all FLOW artifacts — close PRs, delete worktrees/branches/state files/lock entries |
| [`/flow-config`](flow-config.md) | Display current configuration — version and per-skill autonomy |
| [`/flow-skills`](flow-skills.md) | Display the FLOW skill catalog grouped by user role — Maintainer and Private buckets render only inside the FLOW plugin repo |
| [`/flow-doc-sync`](flow-doc-sync.md) | Full codebase documentation accuracy review — reports drift between code and docs |
| [`/flow-hygiene`](flow-hygiene.md) | Audit instruction corpus health — CLAUDE.md, rules, and memory for staleness, misplacement, duplication, and contradictions |
| [`/flow-issues`](flow-issues.md) | Fetch open issues, categorize, prioritize, and display a dashboard with recommended work order. Supports readiness filters (`--ready`, `--blocked`, `--decomposed`, `--quick-start`) and narrowing filters (`--label`, `--milestone`) |
| [`/flow-explore`](flow-explore.md) | Open a problem-statement conversation (PM voice) — discussion-mode by default, files a vanilla `## What` / `## Why` / `## Acceptance Criteria` issue on user signal |
| [`/flow-plan`](flow-plan.md) | Decompose a vanilla problem-statement issue (filed by `/flow-explore`) into a linked decomposed issue ready for the start phase. Tech Lead voice, mandatory `decompose:decompose` pass, files with `--label decomposed` and a blocked-by link to the parent |
| [`/flow-decompose-project`](flow-decompose-project.md) | Decompose a large project into linked GitHub issues with sub-issue relationships, blocked-by dependencies, and milestones |
| [`/flow-orchestrate`](flow-orchestrate.md) | Process decomposed issues sequentially overnight via flow-start --auto |
| [`/flow-triage-issue`](flow-triage-issue.md) | Triage a single open GitHub issue from a PM lens. Reads code, checks for already-shipped work, returns a verdict in `{close, decompose}` |
