---
title: Phase Comparison Reference
nav_order: 10
parent: Reference
---

# Phase Comparison: Metaswarm & Superpowers

This document captures the full phase analysis from both source projects used as inspiration for FLOW Process. Use it to make informed decisions about what to adopt, adapt, or skip.

---

## Metaswarm — 9 Phases

| # | Phase | What it does | BEADS required? |
|---|-------|-------------|-----------------|
| 0 | **Knowledge Priming** | Prime agents with relevant patterns/gotchas from JSONL knowledge base before every task | Yes |
| 1 | **Research** | Codebase analysis, pattern identification, dependency mapping, risk identification | No |
| 2 | **Plan** | Implementation plan with tasks, decisions, schema changes, risks, dependencies | No |
| 2a | **External Dependencies** | Scan plan for required external services, confirm credentials with human | No |
| 2b | **Work Unit Decomposition** | Break plan into discrete work units (WU-001, etc.) with Definition of Done + dependency graph | Yes |
| 3 | **Design Review Gate** | 6 agents in parallel (PM, Architect, Designer, Security, CTO, UX) — ALL must approve | No |
| 4 | **Orchestrated Execution** | Per work unit: Implement → Validate → Adversarial Review → Commit (4-phase loop, max 3 retries) | No |
| 5 | **Final Comprehensive Review** | Full test suite + type check + lint + integration checks across all work units | No |
| 6 | **PR Creation** | Create PR, auto-invoke PR Shepherd | No |
| 7 | **PR Shepherd** | Polls CI every 60s, fixes failures, handles review comments, monitors toward merge | No |
| 8 | **Merge & Closure** | Human approves, squash-merge, close issue | Yes |
| 9 | **Learning & Reflection** | Extract patterns/gotchas/decisions into JSONL knowledge base | Yes |

### Metaswarm — 18 Agents

| Layer | Agents |
|-------|--------|
| Orchestration | Swarm Coordinator, Issue Orchestrator |
| Research & Planning | Researcher, Architect, PM, Designer, Security Design, CTO |
| Implementation | Coder |
| Review & Quality | Code Reviewer, Security Auditor, Test Automator |
| Delivery | PR Shepherd, Knowledge Curator |
| Operations | Metrics, Slack Coordinator, SRE, Customer Service |

### Metaswarm — Critical Invariants

- Adversarial reviewer is always a fresh agent — never reused, never resumed
- Orchestrator runs validation directly — never delegated to the coder
- Max 3 retries per gate, then escalate to human
- All quality gates are blocking — no FAIL → COMMIT path
- File scope verification: `git diff --name-only` after every implementation
- Knowledge priming runs before all agent work

---

## Superpowers — 7 Stages

| # | Stage | What it does |
|---|-------|-------------|
| 1 | **Brainstorming** | Clarify requirements, propose 2-3 alternatives, get design approved — hard gate, no code before approval |
| 2 | **Git Worktrees** | Create isolated workspace with safety verification, run setup, verify clean baseline |
| 3 | **Writing Plans** | Numbered tasks (2-5 min each), affected files, code samples, test commands, commit commands |
| 4a | **Executing Plans** | Batch execution (3 tasks at a time), report, await feedback, iterate |
| 4b | **Subagent-Driven Development** | Fresh subagent per task + two-stage code review (spec compliance → code quality) |
| 5 | **TDD** | RED-GREEN-REFACTOR — test must fail first, watch it fail, no exceptions |
| 6 | **Code Review** | After each task (subagent mode) or before merge — spec compliance + code quality |
| 7 | **Finishing Branch** | Verify tests pass, then present exactly 4 options: merge / push PR / keep / discard |

### Superpowers — Always-On Skills

brainstorming, using-git-worktrees, writing-plans, executing-plans, subagent-driven-development, test-driven-development, systematic-debugging, requesting-code-review, receiving-code-review, verification-before-completion, finishing-a-development-branch, dispatching-parallel-agents

### Superpowers — Critical Invariants

- Brainstorming hard gate: no implementation skill invoked until design is approved
- TDD: test must fail first — "if you didn't watch it fail, you don't know if it tests the right thing"
- Verification before completion: evidence before claims, always
- Finish branch: always verify tests first, always present exactly 4 options
- Subagent-driven: two review stages (spec compliance + code quality) — never skip either

---

## What is BEADS-Dependent (Excluded)

| Feature | Why excluded |
|---------|-------------|
| Knowledge Priming (Metaswarm Phase 0) | Requires `bd prime` CLI |
| Work Unit Decomposition (Phase 2b) | Requires `bd create`, `bd dep` CLI |
| PR tracking + closure (Phase 8) | Requires `bd close` CLI |
| Learning extraction to JSONL (Phase 9) | Requires `.beads/knowledge/` JSONL store |

**FLOW Process replacement:** Learnings are captured as generic Rails patterns written directly to the project's `CLAUDE.md`.

---

## Cherry-Pick Decisions

### From Metaswarm — Adopt

| Concept | Why |
|---------|-----|
| Research phase before implementation | Prevents blind changes; critical for Rails where callbacks and concerns are non-obvious |
| Design review (simplified) | A focused review before coding catches scope/architecture mistakes early |
| Adversarial review in execution loop | Fresh agent, binary PASS/FAIL — independent validation, not self-reporting |
| Orchestrator runs `bin/ci` directly | Don't trust the implementer to self-report green CI |
| Max 3 retries → escalate to human | Prevents infinite loops on genuinely hard problems |
| PR Shepherd concept | CI monitoring and fixing after PR opens |
| Learning → `CLAUDE.md` (not JSONL) | Same value, no tooling dependency |

### From Superpowers — Adopt

| Concept | Why |
|---------|-----|
| Brainstorming hard gate | No code before design approval — even simple things can be complex |
| Worktree safety verification | Prevent accidental commits of worktree directories |
| Numbered time-bounded tasks | Makes plans executable and reviewable |
| Batch execution with checkpoints | Keeps human in the loop without micromanaging |
| TDD as non-negotiable | Aligns with existing Rails test conventions |
| Verification before completion | Evidence before claims — no "it should work" |
| Exactly N options when finishing | Forces explicit decision, prevents drift |

### From Both — Skip

| Concept | Why |
|---------|-----|
| 18 agents | Excessive for a single-developer Rails workflow |
| 6-reviewer parallel design gate | Too heavy; one focused review is sufficient |
| JSONL knowledge base | Tooling overhead; `CLAUDE.md` is simpler and more durable |
| PR Shepherd polling loop | Overkill; developer monitors their own PRs |
| Slack/SRE/Customer Service agents | Out of scope |
| External model support (Codex, Gemini) | Out of scope |
