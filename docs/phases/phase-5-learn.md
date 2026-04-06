---
title: "Phase 5: Learn"
nav_order: 6
---

# Phase 5: Learn

**Command:** `/flow-learn`

Runs before the PR is merged. Audits rule compliance, identifies process
gaps, and creates missing rules. Routes findings to their correct
permanent homes, files GitHub issues for plugin improvements, and
presents a comprehensive report. The only commits are CLAUDE.md and
`.claude/` changes — application code is never touched.

---

## Three Tenants

Learn is an audit, not a retrospective. It asks three specific questions:

1. **Did the FLOW process work?** — identify gaps in the plugin's workflow. These become GitHub issues filed against the plugin repo.
2. **Did Claude follow the rules?** — audit CLAUDE.md and `.claude/rules/` compliance. For each violation, assess whether the rule was unclear (clarify it) or clear but ignored (escalate to HARD-GATE or hook).
3. **What rules should exist but don't?** — create forward-looking rules for undocumented patterns and gaps in coverage.

---

## Sources

Learn gathers artifacts and passes them to the learn-analyst agent for
cognitively isolated analysis:

1. **CLAUDE.md and rules files** — the project's rules and conventions that should have been followed
2. **State file and plan data** — visit counts, timing, captured `/flow-note` entries, plan file risks
3. **Branch diff** — the full `git diff origin/main...HEAD`

All artifacts are passed inline to the learn-analyst agent, which
produces structured findings categorized by the three tenants. The agent
writes findings incrementally — if it exhausts its turn budget, partial
findings are preserved.

---

## What Gets Captured

Claude routes findings autonomously based on content and tenant:

| Destination | What goes here | Write method |
|---|---|---|
| Project CLAUDE.md | Process rules and project architecture | `bin/flow write-rule`, committed via PR |
| `.claude/rules/` | Coding anti-patterns and gotchas | `bin/flow write-rule`, committed via PR |

Both CLAUDE.md and `.claude/rules/` are written via `bin/flow write-rule`
and committed to the feature branch. All edits target the project repo —
never user-level `~/.claude/` paths.

**GitHub issues** — filed during Learn:

- **Process gap** issues — FLOW process gaps, filed on the plugin repo (`benkruger/flow`)
- **Enforcement escalation** issues — rules that were clearly stated but ignored, recommending HARD-GATE or hook enforcement

All filed issues are recorded in the state file via `bin/flow add-issue`
and surfaced in the Complete phase.

---

## What Makes a Good Rule

**Good:** Generic principle that prevents the same class of mistake in any future feature
> "Never assume branch-behind is unlikely in a multi-session workflow"

**Bad:** Feature-specific note that only applies here
> "The payments module uses a specific queue configuration"

---

## Enforcement Escalation

When a rule is violated, Learn assesses the enforcement level:

1. **Rule was unclear** → clarify the rule wording
2. **Rule was clear but ignored** → clarify the rule AND file an enforcement escalation issue (recommend HARD-GATE or hook)

---

## What Comes Next

Run Phase 6: Complete (`/flow-complete`) to merge the PR (which now
includes rule improvements) and clean up.
