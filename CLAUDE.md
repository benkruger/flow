# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase Rails development lifecycle. Skills live in `skills/<name>/SKILL.md`. State lives in `.flow-states/<branch>.json` in the target Rails project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects resume context
- `hooks/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)
- `hooks/extract-release-notes.py` — extracts version sections from RELEASE-NOTES.md for GitHub Releases
- `docs/reference/flow-state-schema.md` — state file schema reference
- `docs/reference/skill-pattern.md` — template pattern for building new phase skills
- `marketplace.json` — marketplace registry (version must match plugin.json)
- `.github/workflows/ci.yml` — GitHub Actions CI (Python 3.12, pytest)

## Development Environment

- Python virtualenv at `.venv/` — `bin/ci` uses `.venv/bin/python3` automatically
- Run tests with `bin/ci` only — never invoke pytest directly
- Dependencies managed in the venv, not system Python

## Architecture

### Plugin vs Target Project

This repo is the plugin source. When installed, skills and hooks run in the target Rails project's working directory. State files live in the target project's `.flow-states/`. Worktrees are created in the target project. Hooks must be tested in the context of a target project directory structure, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown instructions (`skills/<name>/SKILL.md`). The only executable code is `hooks/check-phase.py`, `hooks/extract-release-notes.py`, `hooks/session-start.sh` (with embedded Python), and `bin/ci`. Everything else is instructions that Claude reads and follows.

### State File

The state file (`.flow-states/<branch>.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixture: `tests/conftest.py:make_state()`.

### Sub-Agents

Four phase skills launch mandatory Explore-type sub-agents: Research, Design, Plan, Review. Start uses a general-purpose Sonnet sub-agent for CI failures. Code has no sub-agent. Sub-agent prompts must include a tool restriction rule and must not use Bash for file checks.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

### Version Locations

The version lives in 4 places, all must match: `plugin.json`, `marketplace.json` (top-level metadata), `marketplace.json` (plugins array entry). `test_structural.py` enforces consistency.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files).

| Test File | What It Enforces |
|-----------|------------------|
| `test_structural.py` | Config invariants: phases 1-8 exist, versions match across 4 files, commands unique, hooks reference existing files |
| `test_skill_contracts.py` | SKILL.md content: HARD-GATE presence, announce banners, state updates, sub-agent types, model recommendations, logging sections, note-capture options. Uses glob-based discovery — new skills are automatically covered |
| `test_check_phase.py` | Phase guard: blocks on incomplete prerequisites, allows on complete, handles worktrees, re-entry notes |
| `test_session_start.py` | Session hook: feature detection, timing reset, resume injection, multi-feature handling |
| `test_docs_sync.py` | Docs completeness: every skill has a docs page, every phase has a docs page, index and README mention all commands |
| `test_permissions.py` | Permission coverage: every Bash command in every SKILL.md has coverage in settings.json. Adding a new Bash command to a skill without updating settings.json will fail this test |
| `test_bin_ci.py` | CI runner: venv detection, pass/fail behavior |
| `test_extract_release.py` | Release notes extraction from RELEASE-NOTES.md |

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
- CLAUDE.md changes only through `/reflect` — never edit CLAUDE.md directly. The `/reflect` skill exists to review mistakes, propose additions, get individual approval for each change, and commit. Editing CLAUDE.md outside of `/reflect` bypasses all of that.
- **Never add pymarkdown exclusions** — The `.pymarkdown.yml` disables MD013 (line length), MD025 (multiple H1 with frontmatter), MD033 (inline HTML), and MD036 (emphasis as heading) because those conflict with this repo's intentional patterns. No further rule disablements or path exclusions may be added. If a markdown file triggers a lint error, fix the file — do not suppress the rule. If a rule genuinely cannot be satisfied, surface it to the user for a decision.

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
- **When a fix fails, stop and re-diagnose before pivoting** — If a committed fix doesn't solve the problem, do not immediately start coding a different approach. Stop, explain why the fix failed (or say you don't know), present the new diagnosis and proposed approach, and wait for approval. The session protocol applies every time, not just the first time.
- **Test permission changes before committing** — When adding or changing permission patterns in settings.json, verify the format is valid and the pattern will actually be honored by Claude Code. If you cannot verify (e.g., unsure whether Write(...) is a valid permission type, or whether out-of-project paths are covered), say so and propose a testable alternative rather than committing an unverified fix.
- **Answer completely when asked to explain something** — When the user asks "why does X do Y" or "audit this file", explain every item, not just the interesting ones. Do not dismiss entries as "straightforward" or skip them. If you don't explain it, the user has to ask again.
- **Only gitignore files that exist or will be created in this repo** — Do not add preventive gitignore entries for files that might theoretically appear. If a file does not exist and no process in this repo creates it, it does not belong in .gitignore. Gitignore entries for nonexistent files are clutter.
- **Never fabricate excuses for mistakes** — When you make a mistake, state what you did wrong and stop. Do not invent explanations like "time pressure" or "complexity" to soften it. There is no time pressure. Say what happened, not why it was understandable.
- **Never create symlinks to real binaries in test fixtures** — `Path.write_text()` follows symlinks and overwrites the target. A symlink to `sys.executable` + `write_text()` will corrupt the actual python binary. Use wrapper scripts (`exec <real_path> "$@"`) instead of symlinks when tests need a fake executable at a known path.
- **Trace every fixture operation that touches real system resources** — When a test fixture creates references to real files, binaries, or executables (symlinks, paths from sys.executable, shutil.copy), mentally trace every subsequent operation in the test. If any operation (write_text, open, chmod) could follow a reference back to the real resource and mutate it, the fixture is unsafe. Replace indirect references with self-contained fakes (wrapper scripts, copied binaries) that cannot escape the temp directory.
