# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 6-phase development lifecycle: Start, Plan, Code, Code Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead — you must complete each phase before entering the next. Supports Rails, Python, iOS, Go, and Rust.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets guide every design decision:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and `CLAUDE.md` as project config. `.flow.json` is always git-excluded. Everything else lives in `.git/` or is gitignored.
2. **As autonomous or manual as you want** — configurable autonomy via `.flow.json` skills settings.
3. **Safe for local env** — no containers needed, no permission prompts ever. Native tools only, no external dependencies.
4. **N×N×N concurrent** — N engineers running N flows on N boxes at the same time is the primary use case, not an edge case. Every feature, fix, and design decision must work when multiple flows are active simultaneously — on the same machine (multiple worktrees) and across machines (shared GitHub state). Local state (`.flow-states/`, worktrees) is per-machine. Shared state (PRs, issues, labels) is coordinated through GitHub. Nothing assumes a single active flow.

In the target project:

- `.claude/settings.json` and `CLAUDE.md` are committed during prime. `.flow.json` is always git-excluded
- `.flow-states/` is gitignored and deleted at Complete
- After Complete, the only permanent artifacts are the merged PR and any CLAUDE.md learnings
- Skills are pure Markdown instructions, not executable code
- Framework support is data-driven via `frameworks/<name>/` directories — adding a language means adding a directory, not editing skills
- Multiple flows run simultaneously via branch-scoped worktrees and state files — nothing assumes a single active flow

## The 6 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Create worktree, PR, state file, configure workspace |
| 2 | Plan | `/flow:flow-plan` | Invoke decompose plugin for DAG analysis, explore codebase, create implementation plan |
| 3 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 4 | Code Review | `/flow:flow-code-review` | Six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation) launched in parallel. Parent gathers, triages, and fixes. |
| 5 | Learn | `/flow:flow-learn` | Review mistakes, capture learnings, route to permanent homes |
| 6 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

Phase gates are enforced by `bin/flow check-phase` (`src/check_phase.rs`) — there is no instruction path to skip a phase. Back-transitions (e.g., Code Review can return to Code or Plan) are defined in `flow-phases.json`.

## When You Must Update Docs and Tests

"Marketing docs" refers to `docs/index.html` — the GitHub Pages landing page.

### Structural sync (CI-enforced by `test_docs_sync.py`)

CI will fail if these are missing:

- New/renamed skill — `docs/skills/<name>.md`, `docs/skills/index.md`, `README.md`
- New/renamed phase — `docs/phases/phase-<N>-<name>.md`, `docs/skills/index.md`, `README.md`, `docs/index.html`
- New feature/capability — `README.md` and `docs/index.html` must mention required keywords (see `REQUIRED_FEATURES` in `test_docs_sync.py`)

### Content sync (convention-enforced — no test catches this)

- Changed skill behavior (new flag, changed steps, different workflow) — update `docs/skills/<name>.md` and the Description column in `docs/skills/index.md` to match
- Changed phase behavior — update `docs/phases/phase-<N>-<name>.md` and the Description column in `docs/skills/index.md` to match
- Changed architecture or capabilities — update `README.md` and `docs/index.html` if the change affects how FLOW is described to users

### Test requirements

- New `lib/*.py` script — corresponding `tests/test_*.py` with 100% coverage
- New skills auto-covered by `test_skill_contracts.py` (glob-based discovery)
- Any new executable code needs tests — skills are Markdown and don't need tests beyond contracts

## Key Files

- `config.json` — plugin-level maintainer config: `claude_code_audited` tracks the last Claude Code version audited
- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's Markdown instructions
- `hooks/hooks.json` — hook registration (SessionStart, PreToolUse, PermissionRequest, PostToolUse, PostCompact, Stop, StopFailure)
- `hooks/session-start.sh` — detects in-progress features, injects awareness context
- `.claude/settings.json` — project permissions (git rebase denied)
- `.github/workflows/ci.yml` — GitHub Actions CI (runs `bin/ci` on push/PR to main)
- `.github/workflows/autoupdate.yml` — auto-updates PR branches when main advances
- `docs/` — GitHub Pages site (static HTML); `docs/reference/flow-state-schema.md` for state file schema
- `frameworks/<name>/` — per-framework data: `detect.json`, `permissions.json`, `dependencies`, `priming.md`
- `agents/*.md` — six custom plugin sub-agents: ci-fixer, reviewer, pre-mortem, adversarial, learn-analyst, documentation
- `lib/flow_utils.py` — shared utilities (timestamps, branch detection, state mutation, repo detection)
- `lib/*.py` — utility scripts invoked by `bin/flow` subcommands (read individual files for details)
- `bin/flow` — hybrid dispatcher: tries Rust binary first (`target/release/flow-rs` or `target/debug/flow-rs`), auto-rebuilds when source is newer than binary, falls back to `lib/*.py` on exit 127
- `qa/templates/<framework>/` — per-framework QA repo templates (rails, python, ios, go, rust)
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)

## Development Environment

- Python virtualenv at `.venv/` — `bin/ci` uses `.venv/bin/python3` automatically
- Run tests with `bin/ci` only — never invoke pytest directly
- **Use `bin/test <path>` for targeted test runs during development** — `bin/ci` runs the full suite and is the gate before committing. `bin/test tests/test_specific.py` runs a subset of Python tests via the same venv; `bin/test --rust <filter>` runs a subset of Rust tests via `cargo test <filter>` (e.g. `bin/test --rust hooks` runs every test in `tests/hooks.rs`). Never call pytest or cargo directly — always use one of the two `bin/test` forms.
- `ruff` enforces Python linting (E+F+W+I rules) and formatting at `line-length = 120` — configured in `ruff.toml`, runs inside `bin/ci`
- Dependencies managed in the venv, not system Python

## Architecture

### Plugin vs Target Project

This repo is the plugin source. When installed, skills and hooks run in the target project's working directory. State files live in the target project's `.flow-states/`. Worktrees are created in the target project. Hooks must be tested in the context of a target project directory structure, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown instructions (`skills/<name>/SKILL.md`). The only executable code is `bin/flow` (dispatcher), `lib/*.py` (utility scripts), `hooks/session-start.sh` (with embedded Python), `bin/ci`, and `bin/test`. Everything else is instructions that Claude reads and follows.

### State File

The state file (`.flow-states/<branch>.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixture: `tests/conftest.py:make_state()`.

### Local vs Shared State

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed — each machine has its own |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API — never assume local knowledge of other engineers' state |

The "Flow In-Progress" label on issues is the cross-engineer WIP detection mechanism. See `.claude/rules/concurrency-model.md` for the developer checklist.

### Sub-Agents

Six custom plugin sub-agents in `agents/*.md` — tiered by task complexity: opus (ci-fixer, adversarial), sonnet (reviewer, pre-mortem), haiku (learn-analyst, documentation). Agent frontmatter must only use supported keys (`name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`) — `test_agent_frontmatter_only_supported_keys` enforces this. The global `PreToolUse` hook (`bin/flow hook validate-pretool`) enforces Bash and Agent tool restrictions across all agents. See `.claude/rules/cognitive-isolation.md` for the two-tier context model and debiasing rationale.

Agent `maxTurns` budgets are set in each agent's frontmatter. When adding or modifying an agent's budget, read peer agents' frontmatter to maintain parity between agents with similar scope (e.g. context-rich read-only agents should have comparable budgets).

### Orchestration

`/flow:flow-orchestrate` is a meta-skill that processes decomposed issues overnight. It fetches open issues labeled "Decomposed", filters out "Flow In-Progress" issues, and runs each sequentially via `flow-start --auto`. State is tracked in `.flow-states/orchestrate.json` (a machine-level singleton, not branch-scoped). The session-start hook detects orchestrator state for both in-progress resume and completed morning report delivery. Only one orchestration runs per machine at a time.

### Memory and Learning System

Auto-memory is shared across git worktrees of the same repository (since Claude Code 2.1.63).

Learn routes learnings to project CLAUDE.md and `.claude/rules/`. Also files GitHub issues for process gaps. All filed issues recorded via `bin/flow add-issue`.

Commit always runs `bin/flow ci` before committing — CI handles all run/skip/error logic internally. The `commit_format` preference is copied from `.flow.json` into the state file by `/flow-start`; the commit skill reads it from the state file.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

### Version Locations

The version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` (top-level metadata), `.claude-plugin/marketplace.json` (plugins array entry). `test_structural.py` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of `src/prime_setup.rs`, covering all installation artifacts (hooks, excludes, priming, dependencies). Both hashes are stored in `.flow.json` and compared by `prime_check.rs` when a version mismatch is detected. Matching hashes allow auto-upgrade (just update the version in `.flow.json`); mismatching hashes force a full `/flow:flow-prime` re-run. Hash changes during development do not require version bumps — version bumps are a release decision via `/flow-release`. The hashes ensure that users get the right upgrade path when they update to a new release.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-transition` for entry/completion, `set-timestamp` for mid-phase fields. Exception: `plan-extract` writes Plan phase step fields (`plan_step`, `plan_steps_total`, `files.dag`, `files.plan`, `code_tasks_total`) directly via `mutate_state` when handling the extracted path — these fields are set in the same process that runs phase enter and phase complete. `code_task` can only be incremented by 1 per call — even when multiple tasks are committed atomically, increment the counter one step at a time before committing. The plan file lives at `.flow-states/<branch>-plan.md` and its path is stored in `state["files"]["plan"]`. The DAG file (from decompose plugin) lives at `.flow-states/<branch>-dag.md` and is stored in `state["files"]["dag"]`. Legacy state files may still use top-level `state["plan_file"]` and `state["dag_file"]` — consumers should check `files` first with fallback to the top-level keys.

### Auto-Advance Architecture

Phase auto-advance uses two layers. Layer 1: `phase-transition --action complete` returns `continue_action` (`"invoke"` or `"ask"`) and optionally `continue_target` (the next phase command) in its JSON output. Skill HARD-GATEs parse `continue_action` to decide whether to auto-invoke the next phase or prompt the user. Layer 2: `phase_complete()` writes `_auto_continue` to the state file when `continue_action` is `"invoke"`. The `bin/flow hook validate-ask-user` PreToolUse hook reads `_auto_continue` and auto-answers any `AskUserQuestion` that fires — this is a safety net for cases where the model ignores the HARD-GATE and prompts anyway. `phase_enter()` clears `_auto_continue` when the next phase starts. The `continue_target` field is provided for diagnostic consumers; skills use hardcoded successor commands for reliability.

### Permission Invariant

Every bash block in every skill must run without triggering a permission prompt. `test_permissions.py` enforces at test time (placeholder substitution, allow/deny matching); `bin/flow hook validate-pretool` enforces at runtime via global PreToolUse hook (compound commands, redirection, file-read commands blocked; whitelist enforced when a flow is active). See `.claude/rules/permissions.md` for the pattern-adding protocol.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `target_project` (git repo with non-bash `bin/ci` and no `bin/flow` — simulates a Rails/non-Python target project), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files). Integration tests for lib scripts that run in target projects must use `target_project`, not `git_repo`.

Key test files: `test_structural.py` (config invariants, version consistency), `test_skill_contracts.py` (SKILL.md content via glob-based discovery — new skills auto-covered), `test_permissions.py` (allow/deny simulation, placeholder validation), `test_docs_sync.py` (docs completeness), `test_concurrency.py` (real-process concurrency). Each `tests/test_*.py` corresponds to a `lib/*.py` script with 100% coverage.

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — clone QA repos, prime, run a full lifecycle, and verify results. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature. QA repos exist solely to test the FLOW lifecycle (Start through Complete) — `bin/ci` must run tests only, no linters or style checks. If `bin/ci` fails on seed code, fix the seed, don't debug the linter.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-changelog-audit` — `.claude/skills/flow-changelog-audit/SKILL.md` — audit Claude Code CHANGELOG.md for plugin-relevant changes, categorize as Adopt/Remove/Adapt, file issues

## Conventions

- **Never invoke `/flow-release` unless the user explicitly runs it** — fixing a bug does not authorize a release. Committing a fix and releasing it are separate decisions. The user decides when to ship.
- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts, no "just this once"
- All changes require `bin/flow ci` green before committing — tests are the gate
- New skills are automatically covered by test_skill_contracts.py (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- **Never add pymarkdown exclusions** — The `.pymarkdown.yml` disables MD013 (line length), MD025 (multiple H1 with frontmatter), MD033 (inline HTML), and MD036 (emphasis as heading) because those conflict with this repo's intentional patterns. No further rule disablements or path exclusions may be added. If a markdown file triggers a lint error, fix the file — do not suppress the rule. If a rule genuinely cannot be satisfied, surface it to the user for a decision.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments, no `date -u`. All computation goes through `bin/flow` subcommands. Skills say "run this command", never "calculate this value". `test_skill_contracts.py` enforces this: `test_phase_skills_no_inline_time_computation` fails if any phase skill contains computational instruction patterns.
- **All timestamps use Pacific Time** — `lib/flow_utils.py` provides `now()` which returns `datetime.now(ZoneInfo("America/Los_Angeles")).isoformat(timespec="seconds")`. All scripts import `now` from `flow_utils` — never generate timestamps locally. Existing state files with UTC timestamps (`Z` suffix) are handled by `datetime.fromisoformat()` which parses both formats.
- **Prefer dedicated tools over Bash** — see `.claude/rules/worktree-commands.md`
- **Issue filing** — see `.claude/rules/filing-issues.md`
- **Repo-level targets only** — see `.claude/rules/repo-level-only.md`
- **No `run_in_background` during FLOW phases**; `bin/flow ci` and `bin/ci` are never allowed in the background regardless of mode — see `.claude/rules/ci-is-a-gate.md`. Both enforced by `bin/flow hook validate-pretool`.
- **User evidence is ground truth** — when a user provides screenshots, error output, or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation. Never explain away evidence to preserve your analysis.

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
