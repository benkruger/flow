---
title: /flow-learn
nav_order: 9
parent: Skills
---

# /flow-learn

**Phase:** 5 — Learn

**Usage:** `/flow-learn`, `/flow-learn --auto`, `/flow-learn --manual`, or `/flow-learn --continue-step`

Audits rule compliance, identifies process gaps, and creates missing
rules. Gathers artifacts and passes them to a cognitively isolated
learn-analyst agent, routes findings to CLAUDE.md or `.claude/rules/`,
promotes session permissions, files GitHub issues for plugin
improvements, and presents a comprehensive report. Runs before the PR
merges.

---

## Three Tenants

1. **Did the FLOW process work?** → process gaps → file issues on plugin repo
2. **Did Claude follow the rules?** → compliance audit with enforcement escalation
3. **What rules should exist but don't?** → forward-looking rule creation

---

## Sources

| Source | What | Survives compaction? |
|--------|------|---------------------|
| CLAUDE.md and rules files | Project rules and conventions that should have been followed | Yes |
| State file and plan data | Visit counts, timing, notes, plan risks | Yes |
| Branch diff | Full `git diff origin/main...HEAD` | Yes |
| Learn-analyst agent | Categorized findings from cognitively isolated compliance audit | N/A (agent output) |

All artifacts are passed inline to the learn-analyst agent. The agent
writes findings incrementally — partial findings survive turn budget
exhaustion.

---

## Outputs

Findings are routed autonomously by tenant:

| # | Destination | Path | Method |
|---|-------------|------|--------|
| 1 | Project CLAUDE.md | `CLAUDE.md` in worktree | `bin/flow write-rule` |
| 2 | `.claude/rules/` | `.claude/rules/<topic>.md` in worktree | `bin/flow write-rule` |

Both CLAUDE.md and `.claude/rules/` edits are committed to the feature branch
via `/flow-commit --auto`. All edits target the project repo — never
user-level `~/.claude/` paths.

**Permission promotion** — session permissions accumulated in
`.claude/settings.local.json` are merged into `.claude/settings.json`
via `bin/flow promote-permissions`. The local file is deleted after
merging.

**GitHub issues** — filed during Learn:

- **Process gap** — FLOW process gaps, on the plugin repo (`benkruger/flow`)
- **Enforcement escalation** — rules clearly stated but ignored, recommending HARD-GATE or hook

All filed issues are recorded in the state file via `bin/flow add-issue`.

**Report** — presented after all changes are applied:

- Findings (3 categories matching tenants: process gaps, rule compliance, missing rules)
- Truncated agent (if learn-analyst exhausted its turn budget)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title, tagged by type)

---

## Mode

Mode is configurable via `.flow.json` (default: auto). In auto mode,
permission promotions are applied automatically and the phase transition
advances to Complete without asking.

---

## Gates

- Phase 4: Code Review must be complete
- Only CLAUDE.md and `.claude/` files are committed — never application code
