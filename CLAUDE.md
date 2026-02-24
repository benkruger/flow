# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase Rails development lifecycle. Skills live in `skills/<name>/SKILL.md`. State lives in `.claude/flow-states/<branch>.json` in the target Rails project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects resume context
- `hooks/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)

## What Still Needs Work

- The `flow-phases.json` `can_return_to` values may need tuning after real use

## Maintainer Skills (private to this repo)

- `/commit` — `.claude/skills/commit` — review diff, approve, commit, push
- `/reflect` — `.claude/skills/reflect/SKILL.md` — review session mistakes, propose CLAUDE.md improvements
- `/release` — `.claude/skills/release/SKILL.md` — bump version, tag, push, create GitHub Release

## Conventions

- All commits via `/commit` skill — no exceptions, no shortcuts, no "just this once"
- All changes require `bin/ci` green before committing — tests are the gate
- New skills are automatically covered by test_skill_contracts.py (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)

## Lessons Learned

- **Never bypass `/commit`** — even when the change is small, even when you just used it two commits ago. "All commits via `/commit` skill" is not a guideline, it is a rule. The user had to interrupt mid-commit to stop this.
- **When fixing mistakes, propose the safe variant first** — `git reset --soft` is safe (keeps changes staged). Bare `git reset` is forbidden. Always specify `--soft` and explain why it's non-destructive before asking permission.
- **Consistency audits require comparing the canonical source first** — When reconciling README and docs, start by identifying the canonical example (the marketing page) and grep for every divergence. Do not edit piecemeal and hope you caught everything. The most obvious inconsistency (different feature example names across files) was the one missed.
- **Verify edits against the source of truth before saving** — When fixing an ordering issue, re-read the SKILL.md to confirm the correct order before writing the edit. Editing from memory introduced the exact error being fixed.
- **Always design for concurrent sessions** — Multiple FLOW features can run simultaneously in different worktrees. Any fix involving shared resources (temp files, log files, state) must be scoped by repo and branch. A fixed filename like `/tmp/flow_commit_msg.txt` will be clobbered by parallel sessions. Always ask: what happens if two sessions hit this at the same time?
- **Never improvise outside documented processes** — When the commit process didn't cover large diffs, Claude improvised a shell redirect to `/tmp/` which triggered a permission prompt. The right answer was already available: `git diff --cached --stat` for summaries, and the Read tool on the Bash tool's persisted output file. If a documented process doesn't handle your situation, propose a process change — don't work around it.
- **When shown a bug, read the code and git history before proposing a fix** — When the user reports a bug (especially with screenshots), read the affected files, run `git log` and `git blame` to understand when and why the current code was written, then trace the actual mechanism before suggesting anything. Guessing at fixes without reading the code or history led to three wrong proposals in a row. The global CLAUDE.md rule applies here too: STOP, READ, INVESTIGATE, UNDERSTAND, REPORT, ACT.
- **When inserting a step into a numbered sequence, renumber all subsequent steps** — Never use letter suffixes (2a, 2b) or fractional numbering. Maintain clean sequential integers and update all internal cross-references to the renumbered steps.
- **Test-first for bug fixes** — When a bug is found, write a failing test that reproduces the bug before writing the fix. The failing test proves the bug exists; the fix makes it pass. Do not fix first and add tests afterwards — that inverts the feedback loop and risks writing tests that pass by coincidence.
