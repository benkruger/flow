---
title: "Phase 4: Learn"
nav_order: 5
---

# Phase 4: Learn

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

Learn passes the small artifacts to the learn-analyst agent inline and
hands off the diff as a file path; the agent reads the project rules
itself for cognitively isolated analysis:

1. **CLAUDE.md and rules files** — the project's rules and conventions that should have been followed; the agent reads CLAUDE.md and the full `.claude/rules/` corpus on demand rather than receiving them inline
2. **State file and plan data** — visit counts, timing, captured `/flow-note` entries, plan file risks (passed inline)
3. **Substantive diff** — `git diff origin/<base_branch>...HEAD -w` (whitespace-only changes filtered) captured to `.flow-states/<branch>/substantive-diff.diff` via `bin/flow capture-diff`; the agent Reads the file. If `capture-diff` reports a missing base ref (`origin/<base_branch>` not fetched into the worktree), Step 1 runs a single `git fetch origin <base_branch>` and retries once, halting rather than launching the agent with a missing diff

Keeping the diff and rule corpus out of the prompt keeps it bounded so a
large diff cannot overflow it and starve the audit of findings. The
agent produces structured findings categorized by the three tenants and
writes them incrementally — if it exhausts its turn budget, partial
findings are preserved. When the agent's output omits the
`END-OF-FINDINGS` completion marker, the skill recovers via
partition-and-combine: re-invoking the agent against a narrowed
partition (by tenant or by diff file family) and combining findings
across runs.

---

## What Gets Captured

Claude routes findings autonomously based on content and tenant.
The **obey-vs-describe test** (see
`.claude/rules/persistence-routing.md`) gates every destination
choice: findings that name behavior the model must obey route to
CLAUDE.md or `.claude/rules/`; findings that describe how the system
works route to a module doc comment, the `docs/` subtree, or are
discarded.

| Destination | What goes here | Write method |
|---|---|---|
| Project CLAUDE.md | Behavioral imperatives every session must obey, plus one-line pointer indexes to rule files | `bin/flow write-rule`, committed via PR |
| `.claude/rules/` | Domain-specific behavioral instructions the model obeys | `bin/flow write-rule`, committed via PR |
| Module doc comment in `src/<name>.rs` | Rust code mechanics descriptions | Edit tool + `add-finding --outcome rule_written` |
| `docs/` subtree | Long-form architecture, schema reference, public-facing material | Edit tool + `add-finding --outcome rule_written` |
| Discard | Content the Discoverability test marks derivable from existing code or rules | `add-finding --outcome dismissed` |

Behavioral writes (CLAUDE.md and `.claude/rules/`) route through
`bin/flow write-rule`; descriptive writes (module doc comment,
`docs/`) use the Edit tool directly. All on-main edits are
committed to the feature branch. All edits target the project repo
— never user-level `~/.claude/` paths.

Correction notes captured via `/flow:flow-note` are imperatives by
definition and always route to a CLAUDE.md or `.claude/rules/`
destination — never to module doc, `docs/`, or discard.

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

Run Phase 5: Complete (`/flow-complete`) to merge the PR (which now
includes rule improvements) and clean up.
