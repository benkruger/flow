---
title: /flow-learn
nav_order: 9
parent: Skills
---

# /flow-learn

**Phase:** 4 — Learn

**Usage:** `/flow-learn` or `/flow-learn --continue-step`

Audits rule compliance, identifies process gaps, and creates missing
rules. Gathers artifacts and passes them to a cognitively isolated
learn-analyst agent, routes findings to CLAUDE.md or `.claude/rules/`,
promotes session permissions, files GitHub issues for plugin
improvements, and presents a comprehensive report. Runs before the PR
merges.

On a `--continue-step` resume, the skill first recovers the worktree
directory from a session-keyed anchor (`bin/flow resume-anchor`) so it
re-anchors to the correct working directory before detecting the branch,
even when the working directory drifted between invocations.

---

## Three Tenants

1. **Did the FLOW process work?** → process gaps → file issues on plugin repo
2. **Did Claude follow the rules?** → compliance audit with enforcement escalation
3. **What rules should exist but don't?** → forward-looking rule creation

---

## Sources

| Source | What | Survives compaction? |
|--------|------|---------------------|
| CLAUDE.md and rules files | Project rules and conventions that should have been followed — NOT gathered or passed inline; the agent Globs and Reads `.claude/rules/*.md` and CLAUDE.md itself on demand | N/A (agent reads directly) |
| State file and plan data | Visit counts, timing, notes, plan risks | Yes |
| Correction notes | `state.notes` entries with `type == "correction"`, captured by `/flow:flow-note` — mandatory user directives routed to a durable rule before agent-finding triage | Yes |
| Substantive diff | `git diff origin/<base_branch>...HEAD -w` (whitespace-only changes filtered) captured to `.flow-states/<branch>/substantive-diff.diff` via `bin/flow capture-diff`; the agent Reads the file | Yes |
| Learn-analyst agent | Categorized findings from cognitively isolated compliance audit | N/A (agent output) |

The substantive diff is passed as a file path the learn-analyst agent
Reads; the small artifacts (state file data and plan) are passed
inline; and the agent reads CLAUDE.md and the full `.claude/rules/`
corpus on demand. Keeping the diff and rule corpus out of the prompt
keeps it bounded so a large diff cannot overflow it and starve the
audit of findings. If `capture-diff` reports a missing base ref
(`origin/<base>` not fetched into the worktree), Step 1 runs a single
`git fetch origin <base>` and retries once, halting rather than
launching the agent with a missing diff. When the agent's output omits
the `END-OF-FINDINGS`
completion marker, the skill recovers via partition-and-combine —
re-invoking the agent against a narrowed partition (by tenant or by
diff file family) and combining findings across runs. The agent writes
findings incrementally — partial findings survive turn budget
exhaustion.

---

## Outputs

Findings are routed autonomously by tenant. Findings that name
behavior the model must obey route to CLAUDE.md or `.claude/rules/`;
findings that describe how the system works route to a module doc
comment, the `docs/` subtree, or are discarded per the
**obey-vs-describe test** in `.claude/rules/persistence-routing.md`.

| # | Destination | Path | Method | When |
|---|-------------|------|--------|------|
| 1 | Project CLAUDE.md | `CLAUDE.md` in worktree | `bin/flow write-rule` | Behavioral imperative every session must obey, or a one-line pointer index to a rule file |
| 2 | `.claude/rules/` | `.claude/rules/<topic>.md` in worktree | `bin/flow write-rule` | Domain-specific behavioral instructions the model obeys |
| 3 | Module doc comment | `src/<name>.rs` in worktree | Edit tool + `add-finding --outcome rule_written --path src/<name>.rs` | Rust code mechanics description (descriptive, not behavioral) |
| 4 | `docs/` subtree | `docs/<relative>` in worktree | Edit tool + `add-finding --outcome rule_written --path docs/<relative>` | Long-form architecture, schema reference, public-facing material (descriptive) |
| 5 | Discard | (no write) | `add-finding --outcome dismissed` | Discoverability test resolves negatively — the next session can derive the content from existing code or rules |

Correction notes (mandatory user directives captured via
`/flow:flow-note`) are imperatives by definition; they always route
durably to destination 1 or 2, never to 3, 4, or 5.

All on-main edits are committed to the feature branch via
`/flow-commit`. All edits target the project repo — never
user-level `~/.claude/` paths.

**Permission promotion** — session permissions accumulated in
`.claude/settings.local.json` are merged into `.claude/settings.json`
via `bin/flow promote-permissions`. The local file is deleted after
merging.

**GitHub issues** — filed during Learn:

- **Process gap** — FLOW process gaps, on the plugin repo (`benkruger/flow`)
- **Enforcement escalation** — rules clearly stated but ignored, recommending HARD-GATE or hook

All filed issues are recorded in the state file via `bin/flow add-issue`.
All triage findings (dismissed, rules written/clarified, issues filed)
are recorded via `bin/flow add-finding` for the Complete phase banner.

**Report** — presented after all changes are applied:

- Findings (3 categories matching tenants: process gaps, rule compliance, missing rules)
- Truncated agent (if partition recovery still ended in double-truncation)
- Changes applied (file path + summary for each destination)
- Issues filed (issue number + title, tagged by type)

---

## Mode

Mode is configurable via `.flow.json` (default: auto). In auto mode,
permission promotions are applied automatically and the phase transition
advances to Complete without asking.

---

## Gates

- Phase 3: Review must be complete
- Only CLAUDE.md and `.claude/` files are committed — never application code
