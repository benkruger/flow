# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 6-phase development lifecycle: Start, Plan, Code, Code Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead — you must complete each phase before entering the next. Language-agnostic — every project owns its toolchain via repo-local `bin/format`, `bin/lint`, `bin/build`, `bin/test` scripts that FLOW orchestrates without dispatching by language.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets guide every design decision:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and the four `bin/*` stubs as project config. `.flow.json` is always git-excluded. Everything else lives in `.git/` or is gitignored.
2. **As autonomous or manual as you want** — configurable autonomy via `.flow.json` skills settings.
3. **Safe for local env** — no containers needed, no permission prompts ever. Native tools only, no external dependencies.
4. **N×N×N concurrent** — N engineers running N flows on N boxes at the same time is the primary use case, not an edge case. Every feature, fix, and design decision must work when multiple flows are active simultaneously — on the same machine (multiple worktrees) and across machines (shared GitHub state). Local state (`.flow-states/`, worktrees) is per-machine. Shared state (PRs, issues, labels) is coordinated through GitHub. Nothing assumes a single active flow.

In the target project:

- `.claude/settings.json` and the `bin/*` stubs are committed during prime. `.flow.json` is always git-excluded
- `.flow-states/` is gitignored and deleted at Complete
- After Complete, the only permanent artifacts are the merged PR and any CLAUDE.md learnings
- Skills are pure Markdown instructions, not executable code
- Tool dispatch is repo-local: `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` from cwd. Each repo owns its commands; FLOW provides the orchestration layer (sentinel, retry/flaky classification, recursion guard, fail-fast ordering, JSON contract). Adding a language means writing the four `bin/*` scripts in your project, not editing FLOW
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

### Structural sync (CI-enforced by `tests/docs_sync.rs`)

CI will fail if these are missing:

- New/renamed skill — `docs/skills/<name>.md`, `docs/skills/index.md`, `README.md`
- New/renamed phase — `docs/phases/phase-<N>-<name>.md`, `docs/skills/index.md`, `README.md`, `docs/index.html`
- New feature/capability — `README.md` and `docs/index.html` must mention required keywords (see `required_features()` in `tests/docs_sync.rs`)

### Content sync (convention-enforced — no test catches this)

- Changed skill behavior (new flag, changed steps, different workflow) — update `docs/skills/<name>.md` and the Description column in `docs/skills/index.md` to match
- Changed phase behavior — update `docs/phases/phase-<N>-<name>.md` and the Description column in `docs/skills/index.md` to match
- Changed architecture or capabilities — update `README.md` and `docs/index.html` if the change affects how FLOW is described to users
- New architecturally-unreachable code — add an entry to `test_coverage.md` naming the specific `src/<file>.rs:LINE` coordinates and a one-sentence architectural reason. Every waiver must be tied to a recorded plan-task justification; do not add entries speculatively.

### Test requirements

- New skills auto-covered by `tests/skill_contracts.rs` (glob-based discovery)
- Any new executable code needs tests — skills are Markdown and don't need tests beyond contracts

## Key Files

- `config.json` — plugin-level maintainer config: `claude_code_audited` tracks the last Claude Code version audited
- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's Markdown instructions
- `hooks/hooks.json` — hook registration (SessionStart, PreToolUse, PermissionRequest, PostToolUse, PostCompact, Stop, StopFailure)
- `hooks/session-start.sh` — writes terminal tab colors
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (static HTML); `docs/reference/flow-state-schema.md` for state file schema
- `agents/*.md` — six custom plugin sub-agents: ci-fixer, reviewer, pre-mortem, adversarial, learn-analyst, documentation
- `src/*.rs` — Rust source implementing all `bin/flow` subcommands
- `src/dispatch.rs` — centralized dispatch helpers (`dispatch_json`, `dispatch_text`) for `main.rs` match arms that delegate to module-level `run_impl_main` functions; both helpers print their result and then call `process::exit`
- `bin/flow` — Rust dispatcher: resolves the Rust binary (`target/release/flow-rs` or `target/debug/flow-rs`), auto-rebuilds when source is newer than binary
- `bin/{format,lint,build,test}` — FLOW's own dogfood scripts; each repo gets its own copies installed by `/flow:flow-prime` from `assets/bin-stubs/`
- `assets/bin-stubs/` — self-documenting bash stubs that prime copies into target projects when absent
- `qa/templates/<name>/` — QA repo templates used by `/flow-qa`
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)
- `test_coverage.md` — per-file waiver inventory for lines that `bin/test`'s `--fail-under-*` thresholds cannot reach (architecturally unreachable code: `process::exit` sites, defensive dead branches, subprocess paths requiring real-network dependencies). Each entry names specific `src/<file>.rs:LINE` coordinates and a one-sentence architectural reason.

## Development Environment

- Run tests with `bin/flow ci` only — never invoke cargo directly
- `bin/flow ci` runs `bin/flow format`, `bin/flow lint`, `bin/flow build`, `bin/flow test` in sequence (format first for fail-fast)
- **Use `bin/flow test -- <filter>` for targeted test runs during development** — `bin/flow ci` runs the full suite and is the gate before committing. `bin/flow test -- hooks` runs every test in `tests/hooks.rs`. Never call cargo directly — always use `bin/flow test` or `bin/flow ci`.
- **Full-suite `bin/flow test` rebuilds `flow-rs` from clean; filtered runs stay incremental.** `bin/test` runs `cargo clean -p flow-rs --target-dir target/llvm-cov-target` at the top of the no-filter-args branch so full-suite coverage numbers come from a single source generation. Filtered runs (`bin/flow test -- <filter>`) skip the clean and keep cargo-llvm-cov's `--no-clean` fast path. See "Start-Gate CI on Main as Serialization Point" below for why this matters on main's long-lived target dir.
- Dependencies managed via `bin/dependencies` (runs `cargo update`)

## Architecture

### Plugin vs Target Project

This repo is the plugin source. When installed, skills and hooks run in the target project's working directory. State files live in the target project's `.flow-states/`. Worktrees are created in the target project. Hooks must be tested in the context of a target project directory structure, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown instructions (`skills/<name>/SKILL.md`). The only executable code is `bin/flow` (dispatcher) and `src/*.rs` (Rust source). Everything else is instructions that Claude reads and follows.

### Repo-Local Tool Delegation

`bin/flow ci`, `bin/flow build`, `bin/flow lint`, `bin/flow format`, and `bin/flow test` all spawn `./bin/<tool>` from cwd. The user's `bin/<tool>` script owns the actual command (cargo, pytest, go test, etc.). FLOW contributes:

- Sentinel-based dirty-check optimization (`tree_snapshot` SHA-256 over HEAD + diff + untracked)
- Retry/flaky classification (test only)
- `FLOW_CI_RUNNING=1` recursion guard
- Fail-fast tool ordering (format → lint → build → test)
- Stable JSON output contract
- Cwd-drift guard via `cwd_scope::enforce` so subdirectory-scoped flows can't be run from the wrong directory

The four `bin/*` stubs are installed by `/flow:flow-prime` from `assets/bin-stubs/<tool>.sh` when absent. Pre-existing user scripts are never overwritten. Each stub carries a `# FLOW-STUB-UNCONFIGURED` marker and defaults to `exit 0` with a stderr reminder so a fresh prime never blocks CI. `bin/flow ci` detects the marker in each script's source and refuses to write the sentinel when any tool is still a stub — that way the stderr reminder surfaces on every CI run until the user configures a real command. A repo with no `bin/{format,lint,build,test}` scripts at all (e.g. a subdirectory where prime hasn't run) is a hard error, not a skip: `bin/flow ci` returns `{"status": "error"}` with an actionable message.

### Subdirectory Context

State files capture `relative_cwd` at flow-start time — the path inside the project root where the user invoked `/flow:flow-start`. For root-level flows this is the empty string and behavior is unchanged. For mono-repo flows started inside `api/` (or `packages/api/`), `start-workspace` returns a `worktree_cwd` that includes the suffix so the agent lands in `.worktrees/<branch>/api/` after the worktree is created.

`cwd_scope::enforce` runs as the first action in every subcommand that either runs tools or mutates state: `ci`, `build`, `lint`, `format`, `test` (tool runners) and `phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding` (state mutators). Read-only subcommands (e.g. `format-status`, `tombstone-audit`, `plan-check`) do not enforce because they cannot drift the flow. The guard compares the canonicalized cwd against `<worktree_root>/<relative_cwd>` as a prefix match: cwd must be equal to or a descendant of the expected directory, so agents can cd into sub-modules of `api/` without tripping the guard, but cannot cd into a sibling `ios/` subdirectory. The mechanism is additive: empty `relative_cwd` preserves all pre-existing behavior.

### State File

The state file (`.flow-states/<branch>.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixtures: `tests/common/mod.rs` helpers (`create_git_repo_with_remote`, state JSON builders).

### Local vs Shared State

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed — each machine has its own |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API — never assume local knowledge of other engineers' state |

The "Flow In-Progress" label on issues is the cross-engineer WIP detection mechanism. See `.claude/rules/concurrency-model.md` for the developer checklist.

### Start-Gate CI on Main as Serialization Point

`start-gate` runs `bin/flow ci` on main, under the start lock, by design. This is not a safety check that could live in a worktree — it is the coordination surface for dependency-maintenance work across all concurrent flows.

The pattern: the first flow-start of the day acquires the lock, runs CI on main, and if a dependency upgrade broke something, `ci-fixer` repairs it once, under the lock, with its fix committed to main. Subsequent flow-starts queue behind the lock; when they acquire, main is already repaired, and the CI sentinel (`.flow-states/main-ci-passed`) lets them pass through without re-running CI. Dependency churn costs O(1) human/agent time, not O(N). Running CI in a disposable worktree instead would defeat this: every concurrent flow would independently discover the breakage and independently repair it, producing duplicate fixes, merge conflicts, and wasted cycles.

Consequence: **main's `target/` is a long-lived build surface that spans many source generations as PRs merge over time.** Any tool that writes artifacts under `target/` on main must stay coherent across those generations. cargo-llvm-cov with `--no-clean` is the canonical failure mode — stale instrumented binaries from prior source generations accumulate in `target/llvm-cov-target/debug/deps/`, each with its own embedded coverage map describing an old source layout, and llvm-cov silently merges them into the current report. `bin/test` cleans the `flow-rs` package scope before full-suite runs to prevent this; any future tool that writes coverage-like artifacts on main must enforce the same coherence invariant.

### Sub-Agents

Six custom plugin sub-agents in `agents/*.md` — tiered by task complexity: opus (ci-fixer, adversarial), sonnet (reviewer, pre-mortem), haiku (learn-analyst, documentation). Agent frontmatter must only use supported keys (`name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`) — `test_agent_frontmatter_only_supported_keys` enforces this. The global `PreToolUse` hook (`bin/flow hook validate-pretool`) enforces Bash and Agent tool restrictions across all agents. See `.claude/rules/cognitive-isolation.md` for the two-tier context model and debiasing rationale.

Agent `maxTurns` budgets are set in each agent's frontmatter. When adding or modifying an agent's budget, read peer agents' frontmatter to maintain parity between agents with similar scope (e.g. context-rich read-only agents should have comparable budgets).

### Orchestration

`/flow:flow-orchestrate` is a meta-skill that processes decomposed issues overnight. It fetches open issues labeled "Decomposed", filters out "Flow In-Progress" issues, and runs each sequentially via `flow-start --auto`. State is tracked in `.flow-states/orchestrate.json` (a machine-level singleton, not branch-scoped). Only one orchestration runs per machine at a time.

### Memory and Learning System

Auto-memory is shared across git worktrees of the same repository (since Claude Code 2.1.63).

Learn routes learnings to project CLAUDE.md and `.claude/rules/`. Also files GitHub issues for process gaps. All filed issues recorded via `bin/flow add-issue`. All triage findings recorded via `bin/flow add-finding`.

CI is enforced inside `finalize-commit` itself — `run_impl` calls `ci::run_impl()` before `git commit`, so every commit path (including direct `bin/flow finalize-commit` calls) runs CI mechanically. The sentinel skip optimization means zero overhead when CI already passed. The `commit_format` preference is copied from `.flow.json` into the state file by `/flow-start`; the commit skill reads it from the state file. After `finalize-commit` succeeds and `git pull` did not introduce new content (`pull_merged == false`), the CI sentinel is automatically refreshed so the next `bin/flow ci` run skips when the working tree hasn't changed.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

All 6 phases produce log entries. Phase 1 (Start) logs via its four consolidated commands (`start-init`, `start-gate`, `start-workspace`, `phase-finalize`). Phases 2–5 log via skill-level `bin/flow log` calls and Rust-tier `append_log` calls in `phase_enter.rs` and `finalize_commit.rs`. Phase 6 (Complete) logs via `complete_finalize.rs`, `complete_post_merge.rs`, and `cleanup.rs`. Most log entries use `[Phase N] module — step (status)` format. Phase-transition logs include the action and target phase: `[Phase N] phase-transition --action <action> --phase <phase> ("<status>")`. N is derived from `phase_number()` in `phase_config.rs`. `finalize_commit.rs` reads `current_phase` from the state file to derive the correct phase number across all calling phases. Phase 6 modules use guarded logging to avoid creating `.flow-states/` in test fixtures: `complete_post_merge.rs` and `complete_finalize.rs` check `.flow-states/` directory existence before calling `append_log`; `cleanup.rs` checks log file existence before its final log entry (which is written before the log file deletion step).

### Version Locations

The version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` (top-level metadata), `.claude-plugin/marketplace.json` (plugins array entry). `tests/structural.rs` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of `src/prime_setup.rs`, covering all installation artifacts (hooks, excludes, launcher, bin/* stub installer). Both hashes are stored in `.flow.json` and compared by `prime_check.rs` when a version mismatch is detected. Matching hashes allow auto-upgrade (just update the version in `.flow.json`); mismatching hashes force a full `/flow:flow-prime` re-run. Hash changes during development do not require version bumps — version bumps are a release decision via `/flow-release`. The hashes ensure that users get the right upgrade path when they update to a new release.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-enter` for phase entry (gate check + enter + step counters + state data return — used by Code, Code Review, Learn), `phase-finalize` for phase completion (complete + Slack + notification record — used by Start, Code, Code Review, Learn), `phase-transition` for phases not yet migrated (Plan entry, Complete), `set-timestamp` for mid-phase fields, and `add-finding` for recording triage findings to `findings[]` (used by Code Review and Learn — captures finding description, outcome, reasoning, and optional issue_url or rule path). Exception: `plan-extract` writes Plan phase step fields (`plan_step`, `plan_steps_total`, `files.dag`, `files.plan`, `code_tasks_total`) directly via `mutate_state` when handling the extracted path — these fields are set in the same process that runs phase enter and phase complete. `code_task` can only be incremented by 1 per call — even when multiple tasks are committed atomically, increment the counter one step at a time before committing. The plan file lives at `.flow-states/<branch>-plan.md` and its path is stored in `state["files"]["plan"]`. The DAG file (from decompose plugin) lives at `.flow-states/<branch>-dag.md` and is stored in `state["files"]["dag"]`. Legacy state files may still use top-level `state["plan_file"]` and `state["dag_file"]` — consumers should check `files` first with fallback to the top-level keys.

### Start-Init → Init-State Contract

`start-init` derives the canonical branch name (issue-aware via `fetch_issue_info` + `branch_name`) BEFORE acquiring the start lock. It also computes `relative_cwd` from `cwd.canonicalize().strip_prefix(project_root.canonicalize())` so the captured subdirectory is symlink-stable. It then passes `--branch <canonical> --relative-cwd <rel>` to the `init-state` subprocess, which skips its own derivation and uses the provided values directly. This two-step design ensures the lock is acquired and released under the same name (the canonical branch name). `init-state` retains its full derivation path for backwards compatibility when called directly via `bin/flow init-state` without `--branch`.

### Auto-Advance Architecture

Phase auto-advance uses two layers. Layer 1: the phase completion command (`phase-finalize` for phases 1, 3, 4, 5; `phase-transition --action complete` for phase 2; `complete-finalize` for phase 6) returns `continue_action` (`"invoke"` or `"ask"`) and optionally `continue_target` (the next phase command) in its JSON output. Skill HARD-GATEs parse `continue_action` to decide whether to auto-invoke the next phase or prompt the user. Layer 2: `phase_complete()` writes `_auto_continue` to the state file when `continue_action` is `"invoke"`. The `bin/flow hook validate-ask-user` PreToolUse hook reads `_auto_continue` and auto-answers any `AskUserQuestion` that fires — this is a safety net for cases where the model ignores the HARD-GATE and prompts anyway. Block-first ordering: when the current phase's `phases.<current_phase>.status == "in_progress"` AND the skills config marks it autonomous (`skills.<current_phase>.continue == "auto"`), the same `validate-ask-user` hook returns exit 2 with a rejection message instead of auto-answering — the block path precedes the auto-answer path so the user's explicit continue=auto config wins over any transient boundary state (see `.claude/rules/autonomous-phase-discipline.md`). The `in_progress` scope is load-bearing: after `phase_complete()` advances `current_phase` to the next phase, the next phase's status is still `"pending"` until `phase_enter()` runs, so the completing skill's HARD-GATE prompt to approve the transition is NOT blocked even when the next phase is auto. `phase_enter()` clears `_auto_continue`, `_continue_pending`, and `_continue_context` when the next phase starts. The `continue_target` field is provided for diagnostic consumers; skills use hardcoded successor commands for reliability.

### Permission Invariant

Every bash block in every skill must run without triggering a permission prompt. `tests/permissions.rs` enforces at test time (placeholder substitution, allow/deny matching); `bin/flow hook validate-pretool` enforces at runtime via global PreToolUse hook (compound commands, command substitution, redirection, and file-read commands blocked — the compound and redirect matchers are quote-aware, so operator characters inside single- or double-quoted arguments pass through, and unclosed quotes are pessimistically blocked; whitelist enforced when a flow is active; `general-purpose` sub-agents blocked during active FLOW phases). `bin/flow hook validate-ask-user` additionally blocks `AskUserQuestion` tool calls with exit 2 when the current phase is both in-progress (`phases.<current_phase>.status == "in_progress"`) and autonomous (`skills.<current_phase>.continue == "auto"`) — see `.claude/rules/autonomous-phase-discipline.md` for the motivating incidents and the intentional transition-boundary carve-out. See `.claude/rules/permissions.md` for the pattern-adding protocol.

### Plan-Phase Gates

Phase 2 (Plan) gates completion on two scanners that share `bin/flow plan-check`: `src/scope_enumeration.rs::scan` (universal-coverage prose without a named sibling list) and `src/external_input_audit.rs::scan` (panic/assert tightening proposals without a paired callsite source-classification audit table). Both scanners run at three callsites — the standard path (`src/plan_check.rs::run_impl`), the pre-decomposed extracted path (`src/plan_extract.rs` extracted), and the resume path (`src/plan_extract.rs` resume) — so the gate cannot be bypassed by routing through an alternative phase entry. Each violation in the JSON response carries a `rule` field (`"scope-enumeration"` or `"external-input-audit"`) tying it to the rule file (`.claude/rules/scope-enumeration.md` or `.claude/rules/external-input-audit-gate.md`) the author should consult for the fix. Contract tests in `tests/scope_enumeration.rs` and `tests/external_input_audit.rs` lock the committed prose corpus (CLAUDE.md, `.claude/rules/*.md`, `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) against drift.

### Tombstone Lifecycle

Tombstone tests prevent merge conflicts from silently resurrecting deleted code. The lifecycle has two halves: creation (`.claude/rules/tombstone-tests.md`) and removal (`bin/flow tombstone-audit`). Standalone tombstones (file-existence, source-content checks) live in `tests/tombstones.rs`. Topical tombstones that are integral to a test domain (skill_contracts, structural, dispatcher) stay in their respective test files. The `tombstone-audit` subcommand scans ALL `tests/*.rs` files for PR references, queries GitHub for merge dates, and classifies each as stale or current. Code Review Step 1 runs the audit; Step 4 removes stale tombstones.

## Test Architecture

All tests are Rust integration tests in `tests/*.rs`. Shared helpers in `tests/common/mod.rs` provide `repo_root()`, `bin_dir()`, `hooks_dir()`, `skills_dir()`, `docs_dir()`, `agents_dir()`, `load_phases()`, `load_hooks()`, `plugin_version()`, `phase_order()`, `utility_skills()`, `read_skill()`, `collect_md_files()`, and `create_git_repo_with_remote()`.

Key test files: `tests/structural.rs` (config invariants, version consistency), `tests/skill_contracts.rs` (SKILL.md content via glob-based discovery — new skills auto-covered), `tests/permissions.rs` (allow/deny simulation, placeholder validation), `tests/docs_sync.rs` (docs completeness), `tests/concurrency.rs` (real-process concurrency).

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — clone QA repos, prime, run a full lifecycle, and verify results. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature. QA repos exist solely to test the FLOW lifecycle (Start through Complete) — `bin/flow ci` must run tests only, no linters or style checks. If `bin/flow ci` fails on seed code, fix the seed, don't debug the linter.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-changelog-audit` — `.claude/skills/flow-changelog-audit/SKILL.md` — audit Claude Code CHANGELOG.md for plugin-relevant changes, categorize as Adopt/Remove/Adapt, file issues

## Conventions

- **Never invoke `/flow-release` unless the user explicitly runs it** — fixing a bug does not authorize a release. Committing a fix and releasing it are separate decisions. The user decides when to ship.
- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts, no "just this once". Infrastructure commits during `start-gate` (e.g., `commit_deps` for dependency lock files) are the sole carve-out: they commit directly via Rust under the start lock, before any worktree exists.
- All changes require `bin/flow ci` green before committing — tests are the gate
- New skills are automatically covered by `tests/skill_contracts.rs` (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments, no `date -u`. All computation goes through `bin/flow` subcommands. Skills say "run this command", never "calculate this value". `tests/skill_contracts.rs` enforces this: `phase_skills_no_inline_time_computation` fails if any phase skill contains computational instruction patterns.
- **All timestamps use Pacific Time** — `src/utils.rs` provides `now()` which returns Pacific Time ISO 8601 timestamps. All Rust code uses this function — never generate timestamps via other means.
- **Prefer dedicated tools over Bash** — see `.claude/rules/worktree-commands.md`
- **Issue filing** — see `.claude/rules/filing-issues.md`
- **Repo-level targets only** — see `.claude/rules/repo-level-only.md`
- **Scope enumeration for universal-coverage claims** — see `.claude/rules/scope-enumeration.md`
- **External-input audit for panic/assert tightenings** — see `.claude/rules/external-input-audit-gate.md`
- **No `run_in_background` during FLOW phases**; `bin/flow` (any subcommand) is never allowed in the background regardless of mode — see `.claude/rules/ci-is-a-gate.md`. Enforced by `bin/flow hook validate-pretool`.
- **User evidence is ground truth** — when a user provides screenshots, error output, or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation. Never explain away evidence to preserve your analysis.
