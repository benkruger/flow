# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase development lifecycle. Framework support is data-driven via `frameworks/<name>/` directories — adding a new language means adding a directory, not editing skills. Skills live in `skills/<name>/SKILL.md` and are language-agnostic. State lives in `.flow-states/<branch>.json` in the target project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects awareness context
- `lib/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)
- `lib/extract-release-notes.py` — extracts version sections from RELEASE-NOTES.md for GitHub Releases
- `lib/start-setup.py` — consolidated Start phase setup (git pull, worktree, settings, PR, state file)
- `lib/flow_utils.py` — shared utilities: `now()` (Pacific Time timestamps), `PACIFIC` timezone, `format_time()`, `current_branch()`, `project_root()`, `PHASE_NAMES`, `COMMANDS`
- `lib/phase-transition.py` — phase entry/completion (timing, counters, status, formatted_time)
- `lib/set-timestamp.py` — mid-phase timestamp fields via dot-path notation
- `frameworks/<name>/` — per-framework data: `detect.json`, `permissions.json`, `dependencies`, `priming.md`
- `lib/detect-framework.py` — data-driven framework auto-detection from `frameworks/*/detect.json`
- `lib/prime-project.py` — inserts framework conventions into target CLAUDE.md between markers
- `lib/create-dependencies.py` — copies framework dependency template to `bin/dependencies`
- `bin/flow` — dispatcher script routing subcommands to `lib/*.py`
- `docs/reference/flow-state-schema.md` — state file schema reference
- `docs/reference/skill-pattern.md` — template pattern for building new phase skills
- `marketplace.json` — marketplace registry (version must match plugin.json)
- `.github/workflows/ci.yml` — GitHub Actions CI (Python 3.12, pytest)

## Development Environment

- Python virtualenv at `.venv/` — `bin/ci` uses `.venv/bin/python3` automatically
- Run tests with `bin/ci` only — never invoke pytest directly
- **Use `bin/test <path>` for targeted test runs during development** — `bin/ci` runs the full suite and is the gate before committing. `bin/test tests/test_specific.py` runs a subset via the same venv. Never call pytest directly — always use one of the two wrappers.
- Dependencies managed in the venv, not system Python

## Architecture

### Plugin vs Target Project

This repo is the plugin source. When installed, skills and hooks run in the target project's working directory. State files live in the target project's `.flow-states/`. Worktrees are created in the target project. Hooks must be tested in the context of a target project directory structure, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown instructions (`skills/<name>/SKILL.md`). The only executable code is `bin/flow` (dispatcher), `lib/*.py` (utility scripts), `hooks/session-start.sh` (with embedded Python), `bin/ci`, and `bin/test`. Everything else is instructions that Claude reads and follows.

### State File

The state file (`.flow-states/<branch>.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixture: `tests/conftest.py:make_state()`.

### Sub-Agents

Start uses a sub-agent for CI failures. Plan uses Claude Code's native plan mode (`EnterPlanMode`/`ExitPlanMode`) instead of sub-agents. Review delegates to Claude's built-in `/review` command. Security delegates to Claude's built-in `/security-review` command. Code has no sub-agent.

### Memory and Learning System

Since Claude Code 2.1.63, auto-memory is shared across git worktrees of the same repository. Memory written during feature work persists at the repo-root path and survives worktree cleanup — no rescue needed.

Learning is a unified tri-modal skill. It auto-detects Phase 7 (state file with Security complete), Maintainer (no state file, `flow-phases.json` exists), or Standalone (no state file, no `flow-phases.json`). All three modes route learnings to 5 destinations. Phase 7 adds GitHub issues and phase transitions. Maintainer commits via `/flow:flow-commit --auto`. Standalone never commits.

The 5 destinations split into two types — **instructions** (always loaded, authoritative) and **context** (informational):

- **Instructions (destinations 1-4):** global CLAUDE.md (`~/.claude/CLAUDE.md`), project CLAUDE.md (`CLAUDE.md` in project), global rules (`~/.claude/rules/`), project rules (`.claude/rules/` in project). These tell Claude what to do and not do.
- **Context (destination 5):** project memory (`~/.claude/projects/<repo-root>/memory/MEMORY.md`). This tells Claude what exists, what was decided, and what the user prefers.

Private destinations (1, 3, 5) are written directly outside the repo. Repo destinations (2, 4) are committed via PR (Phase 7) or `/flow:flow-commit --auto` (Maintainer). Notes captured by `/flow:flow-note` feed into the same routing mechanism.

Commit is also tri-modal. It auto-detects FLOW (state file exists), Maintainer (no state file, `flow-phases.json` exists), or Standalone (neither). FLOW mode adds version banners and Python auto-approval. All three modes share the same diff/message/approval/push process.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

### Version Locations

The version lives in 4 places, all must match: `plugin.json`, `marketplace.json` (top-level metadata), `marketplace.json` (plugins array entry). `test_structural.py` enforces consistency.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-transition` for entry/completion, `set-timestamp` for mid-phase fields. The plan file lives at `~/.claude/plans/` (Claude Code's native location) and its path is stored in `state["plan_file"]`.

### Permission Invariant

Every `` ```bash `` block in every skill and docs file must run without triggering a Claude Code permission prompt. `test_permissions.py` enforces this: it extracts every bash block, substitutes placeholders with concrete values, and verifies each command matches an allow-list pattern and does not match a deny-list pattern. New bash commands require a matching permission entry. New placeholders require a `PLACEHOLDER_SUBS` entry. Unrecognized placeholders fail the test — they are never silently skipped.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files).

| Test File | What It Enforces |
|-----------|------------------|
| `test_structural.py` | Config invariants: phases 1-8 exist, versions match across 4 files, commands unique, hooks reference existing files |
| `test_skill_contracts.py` | SKILL.md content: HARD-GATE presence, announce banners, state updates, sub-agent types, model frontmatter, logging sections, note-capture options. Uses glob-based discovery — new skills are automatically covered |
| `test_check_phase.py` | Phase guard: blocks on incomplete prerequisites, allows on complete, handles worktrees, re-entry notes |
| `test_session_start.py` | Session hook: feature detection, timing reset, awareness injection, multi-feature handling |
| `test_docs_sync.py` | Docs completeness: every skill has a docs page, every phase has a docs page, index and README mention all commands |
| `test_permissions.py` | Permission simulation: allow/deny coverage, placeholder validation, source-of-truth sync between init-setup.py and flow-init/SKILL.md, regex unit tests. Unrecognized placeholders fail loudly |
| `test_bin_ci.py` | CI runner: venv detection, pass/fail behavior |
| `test_bin_test.py` | Test runner: venv detection, pass/fail, argument passthrough |
| `test_start_setup.py` | Start setup script: branch naming, settings merge, worktree, state file, logging, error paths |
| `test_phase_transition.py` | Phase entry/completion: timing, counters, status, formatted_time |
| `test_set_timestamp.py` | Mid-phase timestamps: dot-path navigation, NOW replacement |
| `test_extract_release.py` | Release notes extraction from RELEASE-NOTES.md |
| `test_detect_framework.py` | Framework auto-detection: file patterns, multiple matches, defaults, CLI |
| `test_prime_project.py` | CLAUDE.md priming: marker insertion, idempotent replacement, framework switching |
| `test_create_dependencies.py` | Dependency template: file creation, skip-if-exists, chmod, CLI |
| `test_init_setup.py` | Init setup: data-driven permissions, settings merge, version marker, git exclude |

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — `--start`/`--stop` dev mode: uninstall marketplace plugin for local `--plugin-dir` testing, reinstall when done. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-reset` — `.claude/skills/flow-reset/SKILL.md` — remove all FLOW artifacts (worktrees, branches, PRs, state files)

## Conventions

- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts, no "just this once"
- All changes require `bin/flow ci` green before committing — tests are the gate
- New skills are automatically covered by test_skill_contracts.py (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- CLAUDE.md changes only through `/flow:flow-learning` — never edit CLAUDE.md directly. The `/flow:flow-learning` skill exists to review mistakes, propose additions, get individual approval for each change, and commit. Editing CLAUDE.md outside of `/flow:flow-learning` bypasses all of that.
- **Never add pymarkdown exclusions** — The `.pymarkdown.yml` disables MD013 (line length), MD025 (multiple H1 with frontmatter), MD033 (inline HTML), and MD036 (emphasis as heading) because those conflict with this repo's intentional patterns. No further rule disablements or path exclusions may be added. If a markdown file triggers a lint error, fix the file — do not suppress the rule. If a rule genuinely cannot be satisfied, surface it to the user for a decision.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments, no `date -u`. All computation goes through `bin/flow` subcommands. Skills say "run this command", never "calculate this value". `test_skill_contracts.py` enforces this: `test_phase_skills_no_inline_time_computation` fails if any phase skill contains computational instruction patterns.
- **All timestamps use Pacific Time** — `lib/flow_utils.py` provides `now()` which returns `datetime.now(ZoneInfo("America/Los_Angeles")).isoformat(timespec="seconds")`. All scripts import `now` from `flow_utils` — never generate timestamps locally. Existing state files with UTC timestamps (`Z` suffix) are handled by `datetime.fromisoformat()` which parses both formats.
- **Prefer dedicated tools over Bash for all non-execution tasks** — Read files with the Read tool, search with Glob and Grep, create with Write, modify with Edit. Bash should only be used for commands that genuinely require shell execution: `bin/ci`, `bin/test`, `bin/flow`, `make`, and `git`. In this project's strict permission environment (`defaultMode: "plan"`), every Bash command not in the allow list triggers a permission prompt. When you need to explore, understand, or modify files, use dedicated tools — they never prompt.
