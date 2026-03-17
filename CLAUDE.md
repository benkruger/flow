# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 6-phase development lifecycle: Start, Plan, Code, Code Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead — you must complete each phase before entering the next. Supports Rails, Python, and iOS.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Three core tenets guide every design decision:

1. **Unobtrusive** — zero repo footprint, zero dependencies. Nothing is committed — `.claude/settings.json` and `.flow.json` are git-excluded. Everything else lives in `.git/` or is gitignored.
2. **As autonomous or manual as you want** — configurable autonomy via `.flow.json` skills settings.
3. **Safe for local env** — no containers needed, no permission prompts ever. Native tools only, no external dependencies.

In the target project:

- Nothing is committed — `.claude/settings.json` and `.flow.json` are git-excluded
- `.flow-states/` is gitignored and deleted at Complete
- After Complete, the only permanent artifacts are the merged PR and any CLAUDE.md learnings
- Skills are pure Markdown instructions, not executable code
- Framework support is data-driven via `frameworks/<name>/` directories — adding a language means adding a directory, not editing skills

## The 6 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Create worktree, PR, state file, configure workspace |
| 2 | Plan | `/flow:flow-plan` | Invoke decompose plugin for DAG analysis, explore codebase, create implementation plan |
| 3 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 4 | Code Review | `/flow:flow-code-review` | Four lenses: clarity, correctness, safety, CLAUDE.md compliance |
| 5 | Learn | `/flow:flow-learn` | Review mistakes, capture learnings, route to permanent homes |
| 6 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

Phase gates are enforced by `lib/check-phase.py` — there is no instruction path to skip a phase. Back-transitions (e.g., Code Review can return to Code or Plan) are defined in `flow-phases.json`.

## When You Must Update Docs and Tests

### Structural sync (CI-enforced by `test_docs_sync.py`)

CI will fail if these are missing:

- New/renamed skill — `docs/skills/<name>.md`, `docs/skills/index.md`, `README.md`
- New/renamed phase — `docs/phases/phase-<N>-<name>.md`, `docs/skills/index.md`, `README.md`, `docs/index.html`
- New feature/capability — `README.md` and `docs/index.html` must mention required keywords (see `REQUIRED_FEATURES` in `test_docs_sync.py`)

### Content sync (convention-enforced — no test catches this)

- Changed skill behavior (new flag, changed steps, different workflow) — update `docs/skills/<name>.md` to match
- Changed phase behavior — update `docs/phases/phase-<N>-<name>.md` to match
- Changed architecture or capabilities — update `README.md` and `docs/index.html` if the change affects how FLOW is described to users

### Test requirements

- New `lib/*.py` script — corresponding `tests/test_*.py` with 100% coverage
- New skills auto-covered by `test_skill_contracts.py` (glob-based discovery)
- Any new executable code needs tests — skills are Markdown and don't need tests beyond contracts

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — hook registration (SessionStart, PreToolUse)
- `hooks/session-start.sh` — detects in-progress features, injects awareness context
- `lib/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)
- `lib/extract-release-notes.py` — extracts version sections from RELEASE-NOTES.md for GitHub Releases
- `lib/start-lock.py` — serializes concurrent flow-start operations using a file lock at `.flow-states/start.lock` (PID-based stale detection + 30-min timeout)
- `lib/start-setup.py` — consolidated Start phase setup (git pull, worktree, settings, PR, state file)
- `lib/flow_utils.py` — shared utilities: `now()` (Pacific Time timestamps), `PACIFIC` timezone, `format_time()`, `current_branch()`, `project_root()`, `PHASE_NAMES`, `COMMANDS`
- `lib/phase-transition.py` — phase entry/completion (timing, counters, status, formatted_time)
- `lib/set-timestamp.py` — mid-phase timestamp fields via dot-path notation, code_task increment validation (prevents task batching)
- `frameworks/<name>/` — per-framework data: `detect.json`, `permissions.json`, `dependencies`, `priming.md`
- `lib/detect-framework.py` — data-driven framework auto-detection from `frameworks/*/detect.json`
- `lib/prime-project.py` — inserts framework conventions into target CLAUDE.md between markers
- `lib/create-dependencies.py` — copies framework dependency template to `bin/dependencies`
- `agents/ci-fixer.md` — custom plugin sub-agent for CI failure diagnosis and fix
- `lib/finalize-commit.py` — consolidates commit + message-file cleanup + pull + push into one subprocess chain
- `lib/log.py` — appends timestamped entries to `.flow-states/<branch>.log` via Python file append
- `lib/close-issues.py` — closes GitHub issues referenced in the start prompt (`#N` patterns) via `gh issue close`
- `lib/issue.py` — creates GitHub issues via `gh` subprocess (wraps `gh issue create`; auto-detects repo from git remote when `--repo` is omitted)
- `lib/add-issue.py` — records filed issues in the state file's `issues_filed` array (follows `append-note.py` pattern)
- `lib/format-issues-summary.py` — formats `issues_filed` as a markdown table and banner line for Complete phase
- `lib/format-pr-timings.py` — reads state file, formats phase durations as a markdown table for PR body
- `lib/update-pr-body.py` — updates PR body: `--add-artifact` for list items, `--append-section` for collapsible/plain sections
- `lib/stop-continue.py` — Stop hook script that forces continuation when `_continue_pending` flag is set in the state file
- `lib/post-compact.py` — PostCompact hook that captures `compact_summary`, `compact_cwd`, and `compact_count` in the state file for SessionStart to inject
- `lib/validate-ci-bash.py` — global PreToolUse hook validator (blocks compound commands and file-read commands in all Bash calls)
- `lib/validate-ask-user.py` — PreToolUse hook on AskUserQuestion (blocks prompts when `_auto_continue` is set in state file)
- `bin/flow` — dispatcher script routing subcommands to `lib/*.py`
- `docs/reference/flow-state-schema.md` — state file schema reference
- `docs/reference/skill-pattern.md` — template pattern for building new phase skills
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)
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

FLOW uses one custom plugin sub-agent: `ci-fixer` (`agents/ci-fixer.md`) for CI failure diagnosis and fix in Start (Step 2) and Complete (Step 4). Prompt-level tool restrictions are unreliable — sub-agents ignore them. The `PreToolUse` hook (`lib/validate-ci-bash.py`) is registered globally in `hooks/hooks.json`, blocking compound commands and file-read commands in all Bash calls — including those from built-in skills' sub-agents. The ci-fixer also retains its own hook declaration for defense in depth.

Plan invokes the `decompose` plugin (`decompose:decompose`) for DAG-based task decomposition — no plan mode. Code Review delegates to built-in `/simplify`, `/review`, `/security-review`, and the `code-review:code-review` plugin for multi-agent validation. Code and Learn have no sub-agents. Complete uses ci-fixer for CI failures.

### Memory and Learning System

Since Claude Code 2.1.63, auto-memory is shared across git worktrees of the same repository. Memory written during feature work persists at the repo-root path and survives worktree cleanup — no rescue needed.

Learn is a unified tri-modal skill. It auto-detects Phase 5 (state file with Code Review complete), Maintainer (no state file, `flow-phases.json` exists), or Standalone (no state file, no `flow-phases.json`). All three modes route learnings to 2 destinations. Phase 5 adds GitHub issues and phase transitions. Maintainer commits via `/flow:flow-commit --auto`. Standalone never commits.

The 2 destinations:

- **Project CLAUDE.md** (`CLAUDE.md` in project) — process rules, architecture, and conventions. Edited on disk, committed via PR.
- **Project rules** (`.claude/rules/<topic>.md` in project) — coding anti-patterns and gotchas. Filed as "Rule" GitHub issues (not edited directly) to avoid permission prompts that break autonomous flow.

Learn also files GitHub issues for process gaps ("Flow" label on the plugin repo), documentation drift ("Documentation Drift" label), and rule additions ("Rule" label). All filed issues are recorded in the state file via `bin/flow add-issue` and surfaced in the Complete phase.

Code files "Flaky Test" issues when tests fail intermittently during the CI gate. Code Review files "Tech Debt" and "Documentation Drift" issues for out-of-scope findings. All issue filing uses `bin/flow issue` and `bin/flow add-issue`.

Notes captured by `/flow:flow-note` feed into the same routing mechanism.

Commit is also tri-modal. It auto-detects FLOW (state file exists), Maintainer (no state file, `flow-phases.json` exists), or Standalone (neither). FLOW mode adds version banners and Python auto-approval. All three modes share the same diff/message/approval/push process.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

### Version Locations

The version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` (top-level metadata), `.claude-plugin/marketplace.json` (plugins array entry). `test_structural.py` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of the entire `prime-setup.py` file, covering all installation artifacts (hooks, excludes, priming, dependencies). Both hashes are stored in `.flow.json` and compared by `prime-check.py` when a version mismatch is detected. Matching hashes allow auto-upgrade (just update the version in `.flow.json`); mismatching hashes force a full `/flow:flow-prime` re-run. Hash changes during development do not require version bumps — version bumps are a release decision via `/flow-release`. The hashes ensure that users get the right upgrade path when they update to a new release.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-transition` for entry/completion, `set-timestamp` for mid-phase fields. The plan file lives at `.flow-states/<branch>-plan.md` and its path is stored in `state["plan_file"]`. The DAG file (from decompose plugin) lives at `.flow-states/<branch>-dag.md` and is stored in `state["dag_file"]`.

### Permission Invariant

Every `` ```bash `` block in every skill and docs file must run without triggering a Claude Code permission prompt. Two layers enforce this:

- **Test time** — `test_permissions.py` extracts every bash block, substitutes placeholders with concrete values, and verifies each command matches an allow-list pattern and does not match a deny-list pattern. New bash commands require a matching permission entry. New placeholders require a `PLACEHOLDER_SUBS` entry. Unrecognized placeholders fail the test — they are never silently skipped.
- **Runtime** — `validate-ci-bash.py` runs as a global `PreToolUse` hook on every Bash call. It blocks compound commands and file-read commands via fast-path checks, then enforces the `.claude/settings.json` allow list as a whitelist. Commands not matching any `Bash(...)` allow pattern are blocked with exit code 2 and a helpful error message. If `settings.json` is missing (non-FLOW project), the whitelist check is skipped.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `target_project` (git repo with non-bash `bin/ci` and no `bin/flow` — simulates a Rails/non-Python target project), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files). Integration tests for lib scripts that run in target projects must use `target_project`, not `git_repo`.

| Test File | What It Enforces |
|-----------|------------------|
| `test_structural.py` | Config invariants: phases 1-6 exist, versions match across 3 locations, commands unique, hooks reference existing files |
| `test_skill_contracts.py` | SKILL.md content: HARD-GATE presence, announce banners, state updates, ci-fixer agent, logging sections, note-capture options. Uses glob-based discovery — new skills are automatically covered |
| `test_add_issue.py` | Issue recording: append to empty/existing array, missing state file, CLI integration |
| `test_format_issues_summary.py` | Issues summary formatting: empty/missing/single/multiple issues, label grouping, table output, CLI |
| `test_close_issues.py` | Issue closing: extraction of `#N` patterns from prompt, deduplication, partial failure, CLI integration |
| `test_check_phase.py` | Phase guard: blocks on incomplete prerequisites, allows on complete, handles worktrees, re-entry notes |
| `test_session_start.py` | Session hook: feature detection, timing reset, awareness injection, multi-feature handling |
| `test_docs_sync.py` | Docs completeness: every skill has a docs page, every phase has a docs page, index and README mention all commands |
| `test_permissions.py` | Permission simulation: allow/deny coverage, placeholder validation, source-of-truth sync between prime-setup.py and flow-prime/SKILL.md, regex unit tests. Unrecognized placeholders fail loudly |
| `test_bin_ci.py` | CI runner: venv detection, pass/fail behavior |
| `test_bin_test.py` | Test runner: venv detection, pass/fail, argument passthrough |
| `test_start_lock.py` | Start lock: acquire/release/check, stale PID detection, timeout, corrupted lock handling, CLI integration |
| `test_start_setup.py` | Start setup script: branch naming, settings merge, worktree, state file, logging, error paths |
| `test_phase_transition.py` | Phase entry/completion: timing, counters, status, formatted_time |
| `test_set_timestamp.py` | Mid-phase timestamps: dot-path navigation, NOW replacement, code_task increment validation |
| `test_extract_release.py` | Release notes extraction from RELEASE-NOTES.md |
| `test_detect_framework.py` | Framework auto-detection: file patterns, multiple matches, defaults, CLI |
| `test_prime_project.py` | CLAUDE.md priming: marker insertion, idempotent replacement, framework switching |
| `test_create_dependencies.py` | Dependency template: file creation, skip-if-exists, chmod, CLI |
| `test_prime_setup.py` | Prime setup: data-driven permissions, settings merge, version marker, git exclude, pre-commit hook |
| `test_validate_ask_user.py` | AskUserQuestion hook: blocks prompts when `_auto_continue` set, allows when absent/empty, subprocess integration |
| `test_post_compact.py` | PostCompact hook: compact_summary/cwd/count written to state, fail-open on errors, subprocess integration |
| `test_finalize_commit.py` | Commit finalization: happy path, commit/pull/push failures, merge conflict detection, message file cleanup, CLI |
| `test_log.py` | Log append: existing file, new file, directory creation, multiple appends, CLI integration |

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — `--start`/`--stop` dev mode: uninstall marketplace plugin for local `--plugin-dir` testing, reinstall when done. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-reset` — `.claude/skills/flow-reset/SKILL.md` — remove all FLOW artifacts (worktrees, branches, PRs, state files)

## Conventions

- **Never invoke `/flow-release` unless the user explicitly runs it** — fixing a bug does not authorize a release. Committing a fix and releasing it are separate decisions. The user decides when to ship.
- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts, no "just this once"
- All changes require `bin/flow ci` green before committing — tests are the gate
- New skills are automatically covered by test_skill_contracts.py (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- CLAUDE.md changes only through `/flow:flow-learn` — never edit CLAUDE.md directly. The `/flow:flow-learn` skill exists to review mistakes, propose additions, get individual approval for each change, and commit. Editing CLAUDE.md outside of `/flow:flow-learn` bypasses all of that. **Exception:** If a structural test validates that specific content exists in CLAUDE.md (e.g., `test_checksum_version_invariant` validates "Checksum → Version Invariant"), the Code phase may add that content directly to satisfy `bin/flow ci` requirements. The Learn phase reviews this and confirms placement.
- **Never add pymarkdown exclusions** — The `.pymarkdown.yml` disables MD013 (line length), MD025 (multiple H1 with frontmatter), MD033 (inline HTML), and MD036 (emphasis as heading) because those conflict with this repo's intentional patterns. No further rule disablements or path exclusions may be added. If a markdown file triggers a lint error, fix the file — do not suppress the rule. If a rule genuinely cannot be satisfied, surface it to the user for a decision.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments, no `date -u`. All computation goes through `bin/flow` subcommands. Skills say "run this command", never "calculate this value". `test_skill_contracts.py` enforces this: `test_phase_skills_no_inline_time_computation` fails if any phase skill contains computational instruction patterns.
- **All timestamps use Pacific Time** — `lib/flow_utils.py` provides `now()` which returns `datetime.now(ZoneInfo("America/Los_Angeles")).isoformat(timespec="seconds")`. All scripts import `now` from `flow_utils` — never generate timestamps locally. Existing state files with UTC timestamps (`Z` suffix) are handled by `datetime.fromisoformat()` which parses both formats.
- **Prefer dedicated tools over Bash for all non-execution tasks** — Read files with the Read tool, search with Glob and Grep, create with Write, modify with Edit. Bash should only be used for commands that genuinely require shell execution: `bin/ci`, `bin/test`, `bin/flow`, `make`, and `git`. In this project's strict permission environment (`defaultMode: "plan"`), every Bash command not in the allow list triggers a permission prompt. When you need to explore, understand, or modify files, use dedicated tools — they never prompt.
- **Always use `bin/flow issue` to file GitHub issues** — never use `gh issue create` directly. `bin/flow issue` auto-detects the repo from git remote when `--repo` is omitted; pass `--repo` only when filing against a different repo. Direct `gh` calls trigger permission prompts.

<!-- FLOW:BEGIN -->

# Python Conventions

## Architecture Patterns

- **Module structure** — Read the full module and its imports before modifying.
  Check for circular import risks and module-level state.
- **Function signatures** — If modifying a function signature, grep for all
  callers to ensure compatibility.
- **Scripts** — Check argument parsing, error handling, and exit codes. Verify
  the script is registered in any entry points or `bin/` wrappers.

## Test Conventions

- Check `conftest.py` for existing fixtures before creating new ones.
- Never duplicate fixture logic — reuse existing fixtures.
- Follow existing test patterns in the project.
- Targeted test command: `bin/test <tests/path/to/test_file.py>`

## CI Failure Fix Order

1. Lint violations — read the lint output carefully, fix the code
2. Test failures — understand the root cause, fix the code not the test
3. Coverage gaps — write the missing test

## Hard Rules

- Always read module imports before modifying any module.
- Always check `conftest.py` for existing fixtures before creating new ones.
- Never add lint exclusions — fix the code, not the linter configuration.

## Dependency Management

- Run `bin/dependencies` to update packages.

<!-- FLOW:END -->
