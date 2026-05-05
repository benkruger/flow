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
- `src/plan_deviation.rs` — pure detector module for plan signature deviations (plan-named test fixture values drifting in the staged diff). Runs as a post-CI, pre-commit gate inside `src/finalize_commit.rs::run_impl` and blocks the commit when a drift is unacknowledged by a matching `bin/flow log` entry
- `src/dispatch.rs` — centralized dispatch helpers (`dispatch_json`, `dispatch_text`) for `main.rs` match arms that delegate to module-level `run_impl_main` functions; both helpers print their result and then call `process::exit`
- `src/base_branch_cmd.rs` — `bin/flow base-branch` subcommand. Skill-side single source of truth for the integration branch the flow coordinates against — reads `state["base_branch"]` via `git::read_base_branch` and prints the value (no silent fallback). Skills shell into this subcommand instead of hardcoding `origin/main` so non-main-trunk repos coordinate against their actual integration branch.
- `src/tui.rs` — interactive TUI (`flow tui`) — ratatui app with keyboard-driven navigation over local state files. Subprocess surface (open, osascript, bin/flow, $HOME) is injected via `TuiAppPlatform` so unit tests exercise real `Command::new()` chains against a no-op `true` binary
- `src/tui_data.rs` — state-file loaders and pure display helpers for the TUI (phase timeline, flow summaries, orchestration, account metrics, per-phase token table). No IO in the helpers — `phase_timeline`, `phase_token_table`, and friends are deterministic given state JSON + clock
- `src/tui_terminal.rs` — crossterm glue for the production TUI entry point. Hosts `run_tui_arm` (returns `!`, owns the `process::exit` dispatch so main.rs's Tui arm is a single fully-covered expression), `run_tui_arm_impl` (seam-injected variant accepting `is_tty_fn` and `run_terminal_fn` closures for unit tests), `run_terminal` (the crossterm event loop), and `TerminalGuard<F>` (RAII guard parameterized over a `release_fn` closure so unit tests verify Drop runs via `panic::catch_unwind`). See `.claude/rules/rust-patterns.md` "Seam-injection variant for externally-coupled code" and `.claude/rules/panic-safe-cleanup.md`.
- `src/window_snapshot.rs` — pure capture function reading `~/.claude/rate-limits.json`, transcript JSONL, and the per-session cost file to produce a `WindowSnapshot` of account-window pcts, session token totals (with per-model split), session cost, turn/tool counts, and most-recent-turn context utilization. Every numeric field is `Option<...>` for fail-open semantics — missing inputs produce `None`, never panics. Called by every state-mutating producer (`start_init`, `phase_enter`, `phase_finalize`, `phase_transition`, `set_timestamp` step counters, `complete_finalize`) to land snapshots at `state.window_at_start`, `phases.<n>.window_at_enter` / `window_at_complete`, `phases.<n>.step_snapshots[]`, and `state.window_at_complete`.
- `src/window_deltas.rs` — pure delta + by-model rollup helper consuming `WindowSnapshot` data captured in state. Three public functions: `phase_delta(phase: &PhaseState) -> Option<DeltaReport>` for per-phase deltas, `flow_total(state: &FlowState) -> DeltaReport` for flow-level sums, and `by_model_rollup(state: &FlowState) -> IndexMap<String, ModelTokens>`. Cross-session math is load-bearing: snapshots are grouped by `session_id`, deltas summed within each group, then summed across groups so multi-session flows (resume, hand-off after compaction) report non-negative deltas. Window resets (curr pct < prev pct) produce `None` pct deltas and set `window_reset_observed`. Consumed by `format_complete_summary` (Token Cost section), `format_status` (Tokens block), and `tui_data::phase_token_table` (TUI per-phase table).
- `bin/flow` — Rust dispatcher: resolves the Rust binary (`target/release/flow-rs` or `target/debug/flow-rs`), auto-rebuilds when source is newer than binary
- `bin/{format,lint,build,test}` — FLOW's own dogfood scripts; each repo gets its own copies installed by `/flow:flow-prime` from `assets/bin-stubs/`
- `assets/bin-stubs/` — self-documenting bash stubs that prime copies into target projects when absent
- `qa/templates/<name>/` — QA repo templates used by `/flow-qa`
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)

## Development Environment

- Run tests with `bin/flow ci` only — never invoke cargo directly
- `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` in sequence (format first for fail-fast). In THIS repo, `bin/build` is a no-op that exits 0 with an informational stderr message — compilation happens inside `bin/test` via `cargo-llvm-cov nextest`, so a separate build step would duplicate work. Target projects that need a real build step implement their own `bin/build`.
- `bin/flow ci --format`/`--lint`/`--build`/`--test` runs only that single phase. Single-phase runs disable both sentinel read and write — one tool passing does not satisfy the all-four-passed contract the sentinel encodes.
- `bin/flow ci --force` runs all four AND bypasses the sentinel skip
- **For single-file coverage iteration, use `bin/test tests/<name>.rs`** — runs only that test binary and asserts 100/100/100 against the mirrored `src/<name>.rs`. Completes in seconds vs ~3 minutes for full CI. Use full `bin/flow ci` (or `bin/flow ci --test`) only for final cross-file verification before commit. See `.claude/rules/per-file-coverage-iteration.md`.
- **Use `bin/flow ci --test -- <filter>` for targeted test runs across the full workspace** — `bin/flow ci --test -- hooks` runs every test name-matching `hooks` across all test binaries. This still compiles the whole workspace, so prefer `bin/test tests/<name>.rs` when the target is a single file. Never call cargo directly — always go through `bin/test` or `bin/flow ci`.
- **`bin/test` sweeps `*.profraw` recursively under `target/llvm-cov-target/` at the start of every invocation** (full-suite, filtered, forced). llvm-cov's report draws only from the profraws produced by THIS run, so stale instrumented binaries in `target/llvm-cov-target/debug/deps/` contribute no execution counts regardless of `--no-clean`. `bin/flow ci --clean` is the user-facing reset for a full fresh-clone experience when a deeper purge is wanted.
- **Stale instrumented binaries can produce phantom coverage misses.** The profraw sweep removes execution-count data, but the binaries themselves stay in `target/llvm-cov-target/debug/deps/` and their function-instantiation maps remain in cargo-llvm-cov's report denominator. A file can read e.g. `92.31% / 95.54% / 96.15%` with mysterious "missed functions" that resist every test you add — those are stale crate hashes' empty entries, not real source-level gaps. **Diagnostic: when adding tests doesn't move coverage at all, run `bin/flow ci --clean` once** (12s reset + ~45s next-test cold compile), then re-run the per-file gate. See `.claude/rules/per-file-coverage-iteration.md` "Phantom Misses" for the full diagnostic procedure including `bin/test --funcs <file>` for confirmation.
- **`bin/test --show <file>`** renders annotated source coverage for ONE source file. Pass basename (`start_init.rs`) or path (`src/start_init.rs`). **`bin/test --funcs <file>`** lists every function instantiation with its execution count and mangled name — the way to confirm phantom-miss diagnosis (multiple crate hashes for the same demangled name).
- Dependencies managed via `bin/dependencies` (runs `cargo update`)

## Architecture

### Plugin vs Target Project

This repo is the plugin source. When installed, skills and hooks run in the target project's working directory. State files live in the target project's `.flow-states/`. Worktrees are created in the target project. Hooks must be tested in the context of a target project directory structure, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown instructions (`skills/<name>/SKILL.md`). The only executable code is `bin/flow` (dispatcher) and `src/*.rs` (Rust source). Everything else is instructions that Claude reads and follows.

### Repo-Local Tool Delegation

`bin/flow ci` (and its single-phase variants `--format`/`--lint`/`--build`/`--test`) spawns `./bin/<tool>` from cwd. The user's `bin/<tool>` script owns the actual command (cargo, pytest, go test, etc.). FLOW contributes:

- Sentinel-based dirty-check optimization (`tree_snapshot` SHA-256 over HEAD + diff + untracked)
- Retry/flaky classification (test only)
- `FLOW_CI_RUNNING=1` recursion guard
- Fail-fast tool ordering (format → lint → build → test)
- Stable JSON output contract
- Cwd-drift guard via `cwd_scope::enforce` so subdirectory-scoped flows can't be run from the wrong directory

The four `bin/*` stubs are installed by `/flow:flow-prime` from `assets/bin-stubs/<tool>.sh` when absent. Pre-existing user scripts are never overwritten. Each stub carries a `# FLOW-STUB-UNCONFIGURED` marker and defaults to `exit 0` with a stderr reminder so a fresh prime never blocks CI. `bin/flow ci` detects the marker in each script's source and refuses to write the sentinel when any tool is still a stub — that way the stderr reminder surfaces on every CI run until the user configures a real command. A repo with no `bin/{format,lint,build,test}` scripts at all (e.g. a subdirectory where prime hasn't run) is a hard error, not a skip: `bin/flow ci` returns `{"status": "error"}` with an actionable message.

### Subdirectory Context

State files capture `relative_cwd` at flow-start time — the path inside the project root where the user invoked `/flow:flow-start`. For root-level flows this is the empty string and behavior is unchanged. For mono-repo flows started inside `api/` (or `packages/api/`), `start-workspace` returns a `worktree_cwd` that includes the suffix so the agent lands in `.worktrees/<branch>/api/` after the worktree is created. `worktree_cwd` is an **absolute path** so the skill's `cd <worktree_cwd>` lands correctly regardless of where the bash tool's cwd is at the moment of the cd — the user can launch Claude at the repo root and `cd <app>` before `/flow:flow-start`, or launch Claude directly inside the app subdir; both produce the same downstream behavior. `prime_check` reads `.flow.json` from the project root (not from cwd), so a mono-repo primed at the root passes prime-check from any app subdirectory.

Worktree creation also mirrors every `.venv` discovered under the project root into the new worktree as a relative symlink — root-level `.venv` plus every subdirectory `.venv` (`cortex/.venv`, `packages/api/.venv`, etc.). The walker skips dotted directories other than `.venv` itself, a small named-skip list (`node_modules`, `target`, `vendor`, `build`, `dist`), and directory symlinks (cycle protection); pre-existing committed content at a link path is preserved, never overwritten. See `src/start_workspace.rs::link_venvs`.

`cwd_scope::enforce` runs as the first action in every subcommand that either runs tools or mutates state: `ci`, `build`, `lint`, `format`, `test` (tool runners) and `phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding` (state mutators). Read-only subcommands (e.g. `format-status`, `tombstone-audit`, `plan-check`, `base-branch`) do not enforce because they cannot drift the flow. The guard compares the canonicalized cwd against `<worktree_root>/<relative_cwd>` as a prefix match: cwd must be equal to or a descendant of the expected directory, so agents can cd into sub-modules of `api/` without tripping the guard, but cannot cd into a sibling `ios/` subdirectory. The mechanism is additive: empty `relative_cwd` preserves all pre-existing behavior.

### State File

The state file (`.flow-states/<branch>/state.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixtures: `tests/common/mod.rs` helpers (`create_git_repo_with_remote`, state JSON builders).

### Local vs Shared State

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed — each machine has its own |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API — never assume local knowledge of other engineers' state |

The "Flow In-Progress" label on issues is the cross-engineer WIP detection mechanism. See `.claude/rules/concurrency-model.md` for the developer checklist.

### Start-Gate CI on the Base Branch as Serialization Point

`start-gate` runs `bin/flow ci` on the integration branch (the `base_branch` value captured at flow-start by `init_state` from `git symbolic-ref --short refs/remotes/origin/HEAD` — `main` for standard repos, `staging`/`develop`/etc. for non-main-trunk repos), under the start lock, by design. This is not a safety check that could live in a worktree — it is the coordination surface for dependency-maintenance work across all concurrent flows.

The pattern: the first flow-start of the day acquires the lock, runs CI on the base branch, and if a dependency upgrade broke something, `ci-fixer` repairs it once, under the lock, with its fix committed to the base branch. Subsequent flow-starts queue behind the lock; when they acquire, the base branch is already repaired, and the CI sentinel (`.flow-states/<base_branch>-ci-passed`, branch-keyed via `ci::sentinel_path(repo, base_branch)`) lets them pass through without re-running CI. Dependency churn costs O(1) human/agent time, not O(N). Running CI in a disposable worktree instead would defeat this: every concurrent flow would independently discover the breakage and independently repair it, producing duplicate fixes, merge conflicts, and wasted cycles.

Consequence: **the base branch's `target/` is a long-lived build surface that spans many source generations as PRs merge over time.** Any tool that writes artifacts under `target/` on the base branch must stay coherent across those generations. `bin/test` enforces coherence by sweeping every `*.profraw` under `target/llvm-cov-target/` at the start of every invocation: llvm-cov's report is scoped to the profraws produced by THIS run, so stale instrumented binaries left behind in `deps/` from prior generations produce no execution-count contributions. `bin/flow ci --clean` is the user-facing deep-reset when a full fresh-clone experience is wanted.

### Sub-Agents

<!-- duplicate-test-coverage: not-a-new-test -->
Six custom plugin sub-agents in `agents/*.md` — tiered by task complexity: opus (ci-fixer, adversarial), sonnet (reviewer, pre-mortem), haiku (learn-analyst, documentation). Agent frontmatter must only use supported keys (`name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`) — `test_agent_frontmatter_only_supported_keys` enforces this. The global `PreToolUse` hook (`bin/flow hook validate-pretool`) enforces Bash and Agent tool restrictions across all agents. See `.claude/rules/cognitive-isolation.md` for the two-tier context model and debiasing rationale.

Agent `maxTurns` budgets are set in each agent's frontmatter. When adding or modifying an agent's budget, read peer agents' frontmatter to maintain parity between agents with similar scope (e.g. context-rich read-only agents should have comparable budgets).

### Orchestration

`/flow:flow-orchestrate` is a meta-skill that processes decomposed issues overnight. It fetches open issues labeled "Decomposed", filters out "Flow In-Progress" issues, and runs each sequentially via `flow-start --auto`. State is tracked in `.flow-states/orchestrate.json` (a machine-level singleton, not branch-scoped). Only one orchestration runs per machine at a time.

### Memory and Learning System

Auto-memory is shared across git worktrees of the same repository (since Claude Code 2.1.63).

Learn routes learnings to project CLAUDE.md and `.claude/rules/`. Also files GitHub issues for process gaps. All filed issues recorded via `bin/flow add-issue`. All triage findings recorded via `bin/flow add-finding`.

CI is enforced inside `finalize-commit` itself — `run_impl` calls `ci::run_impl()` before `git commit`, so every commit path (including direct `bin/flow finalize-commit` calls) runs CI mechanically. The sentinel skip optimization means zero overhead when CI already passed. The `commit_format` preference is copied from `.flow.json` into the state file by `/flow-start`; the commit skill reads it from the state file. After `finalize-commit` succeeds and `git pull` did not introduce new content (`pull_merged == false`), the CI sentinel is automatically refreshed so the next `bin/flow ci` run skips when the working tree hasn't changed.

Plan signature deviations are enforced the same way. After `ci::run_impl()` succeeds and before the `git commit` call, `run_impl` invokes `crate::plan_deviation::run_impl` which reads the plan file's `## Tasks` section, collects `(test_name, fixture_key, plan_value)` triples from eligible fenced code blocks, and cross-references each plan value against the string literals found in the corresponding diff-added test function's body. A deviation is reported when a plan-named test appears in `git diff --cached` but the plan's fixture value is absent from the test's added lines. When drift is detected and not acknowledged by a matching `bin/flow log` entry (a log line containing both the test name and the plan value as substrings), `finalize-commit` blocks with `{"status": "error", "step": "plan_deviation"}` on stdout and a structured stderr message naming each deviation and the exact `bin/flow log` command the user should run to acknowledge it. See `.claude/rules/plan-commit-atomicity.md` "Mechanical Enforcement" for the full gate design.

### Logging

Phase skills log completion events to `.flow-states/<branch>/log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

All 6 phases produce log entries. Phase 1 (Start) logs via its four consolidated commands (`start-init`, `start-gate`, `start-workspace`, `phase-finalize`). Phases 2–5 log via skill-level `bin/flow log` calls and Rust-tier `append_log` calls in `phase_enter.rs` and `finalize_commit.rs`. Phase 6 (Complete) logs via `complete_finalize.rs`, `complete_post_merge.rs`, and `cleanup.rs`. Most log entries use `[Phase N] module — step (status)` format. Phase-transition logs include the action and target phase: `[Phase N] phase-transition --action <action> --phase <phase> ("<status>")`. N is derived from `phase_number()` in `phase_config.rs`. `finalize_commit.rs` reads `current_phase` from the state file to derive the correct phase number across all calling phases. Phase 6 modules use guarded logging to avoid creating `.flow-states/` in test fixtures: `complete_post_merge.rs` and `complete_finalize.rs` check `.flow-states/` directory existence before calling `append_log`; `cleanup.rs` checks log file existence before its final log entry (which is written before the log file deletion step).

### Version Locations

The version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` (top-level metadata), `.claude-plugin/marketplace.json` (plugins array entry). `tests/structural.rs` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of `src/prime_setup.rs`, covering all installation artifacts (hooks, excludes, launcher, bin/* stub installer). Both hashes are stored in `.flow.json` and compared by `prime_check.rs` when a version mismatch is detected. Matching hashes allow auto-upgrade (just update the version in `.flow.json`); mismatching hashes force a full `/flow:flow-prime` re-run. Hash changes during development do not require version bumps — version bumps are a release decision via `/flow-release`. The hashes ensure that users get the right upgrade path when they update to a new release.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-enter` for phase entry (gate check + enter + step counters + state data return — used by Code, Code Review, Learn), `phase-finalize` for phase completion (complete + Slack + notification record — used by Start, Code, Code Review, Learn), `phase-transition` for phases not yet migrated (Plan entry, Complete), `set-timestamp` for mid-phase fields, and `add-finding` for recording triage findings to `findings[]` (used by Code Review and Learn — captures finding description, outcome, reasoning, and optional issue_url or rule path). Exception: `plan-extract` writes Plan phase step fields (`plan_step`, `plan_steps_total`, `files.dag`, `files.plan`, `code_tasks_total`) directly via `mutate_state` when handling the extracted path — these fields are set in the same process that runs phase enter and phase complete. `code_task` can only be incremented by 1 per `--set` argument — `apply_updates` validates each `--set code_task=N` sequentially against the in-memory state mutated by preceding args, so `--set code_task=2 --set code_task=3` in a single CLI call succeeds (each step is +1). For atomic commit groups, batch all counter advances in one call instead of N separate invocations. The plan file lives at `.flow-states/<branch>/plan.md` and its path is stored in `state["files"]["plan"]`. The DAG file (from decompose plugin) lives at `.flow-states/<branch>/dag.md` and is stored in `state["files"]["dag"]`. Legacy state files may still use top-level `state["plan_file"]` and `state["dag_file"]` — consumers should check `files` first with fallback to the top-level keys.

Account-window snapshots are captured at every state-mutating transition by `src/window_snapshot.rs::capture` — at flow start (`state.window_at_start`), at every phase enter and phase complete (`phases.<n>.window_at_enter` / `window_at_complete`), at every step counter increment (`phases.<n>.step_snapshots[]`), and at flow complete (`state.window_at_complete`). Each snapshot records account-window pcts (5h, 7d), session token totals with per-model split, session cost, turn/tool counts, and most-recent-turn context utilization. Every numeric field is `Option<...>` for fail-open semantics — missing or unreadable inputs produce `None` rather than failing the surrounding state mutation. Consumers (`format_complete_summary` Token Cost section, `format_status` Tokens block, `tui_data::phase_token_table`) read the snapshots through `src/window_deltas.rs` which groups by `session_id` so multi-session flows (resume, hand-off after compaction) report non-negative deltas.

### Start-Init → Init-State Contract

`start-init` derives the canonical branch name (issue-aware via `fetch_issue_info` + `branch_name`) BEFORE acquiring the start lock. It also computes `relative_cwd` from `cwd.canonicalize().strip_prefix(project_root.canonicalize())` so the captured subdirectory is symlink-stable. It then passes `--branch <canonical> --relative-cwd <rel>` to the `init-state` subprocess, which skips its own derivation and uses the provided values directly. This two-step design ensures the lock is acquired and released under the same name (the canonical branch name). `init-state` retains its full derivation path for backwards compatibility when called directly via `bin/flow init-state` without `--branch`.

### Auto-Advance Architecture

Phase auto-advance uses two layers. Layer 1: the phase completion command (`phase-finalize` for phases 1, 3, 4, 5; `phase-transition --action complete` for phase 2; `complete-finalize` for phase 6) returns `continue_action` (`"invoke"` or `"ask"`) and optionally `continue_target` (the next phase command) in its JSON output. Skill HARD-GATEs parse `continue_action` to decide whether to auto-invoke the next phase or prompt the user. Layer 2: `phase_complete()` writes `_auto_continue` to the state file when `continue_action` is `"invoke"`. The `bin/flow hook validate-ask-user` PreToolUse hook reads `_auto_continue` and auto-answers any `AskUserQuestion` that fires — this is a safety net for cases where the model ignores the HARD-GATE and prompts anyway. Block-first ordering: when the current phase's `phases.<current_phase>.status == "in_progress"` AND the skills config marks it autonomous (`skills.<current_phase>.continue == "auto"`), the same `validate-ask-user` hook returns exit 2 with a rejection message instead of auto-answering — the block path precedes the auto-answer path so the user's explicit continue=auto config wins over any transient boundary state (see `.claude/rules/autonomous-phase-discipline.md`). The `in_progress` scope is load-bearing: after `phase_complete()` advances `current_phase` to the next phase, the next phase's status is still `"pending"` until `phase_enter()` runs, so the completing skill's HARD-GATE prompt to approve the transition is NOT blocked even when the next phase is auto. `phase_enter()` clears `_auto_continue`, `_continue_pending`, and `_continue_context` when the next phase starts. The `continue_target` field is provided for diagnostic consumers; skills use hardcoded successor commands for reliability.

### Permission Invariant

Every bash block in every skill must run without triggering a permission prompt. `tests/permissions.rs` enforces at test time (placeholder substitution, allow/deny matching); `bin/flow hook validate-pretool` enforces at runtime via global PreToolUse hook (compound commands, command substitution, redirection, and file-read commands blocked — the compound and redirect matchers are quote-aware, so operator characters inside single- or double-quoted arguments pass through, and unclosed quotes are pessimistically blocked; whitelist enforced when a flow is active; `general-purpose` sub-agents blocked during active FLOW phases). The same hook's Layer 10 mechanically blocks direct commit invocations (`git ... commit`, `bin/flow ... finalize-commit`, with first-token dequoting, `bash -c`/`sh -c` unwrapping, and skip-past `git -c k=v` / `git -C path` flags) when the hook's effective cwd or any `-C` target either resolves to the integration branch named by `default_branch_in` OR resolves to a feature branch with an active FLOW state file at `.flow-states/<branch>/state.json`. The active-flow context carries a skill-commit carve-out: `bin/flow ... finalize-commit` (and only that shape — never `git commit`) passes through when the state file has `_continue_pending == "commit"`, the marker the flow-code, flow-code-review, and flow-learn skills set immediately before invoking `/flow:flow-commit`. The integration-branch context is NOT carved out. See `.claude/rules/concurrency-model.md` "Mechanical Enforcement" for the bypasses defeated, the active-flow trigger, the carve-out's trust contract, and the documented v1 gaps (env-var indirection, git aliases, `xargs`-style indirection, and other unknown launchers). `bin/flow hook validate-ask-user` additionally blocks `AskUserQuestion` tool calls with exit 2 when the current phase is both in-progress (`phases.<current_phase>.status == "in_progress"`) and autonomous (`skills.<current_phase>.continue == "auto"`) — see `.claude/rules/autonomous-phase-discipline.md` for the motivating incidents and the intentional transition-boundary carve-out. See `.claude/rules/permissions.md` for the pattern-adding protocol.

### Plan-Phase Gates

Phase 2 (Plan) gates completion on three scanners that share `bin/flow plan-check`: `src/scope_enumeration.rs::scan` (universal-coverage prose without a named sibling list), `src/external_input_audit.rs::scan` (panic/assert tightening proposals without a paired callsite source-classification audit table), and `src/duplicate_test_coverage.rs::scan` (proposed test names that normalize — strip `test_` prefix, lowercase — to an existing test in the repo's test corpus from `tests/**/*.rs` only; `src/*.rs` inline tests are prohibited by `.claude/rules/test-placement.md`). All three scanners run at three callsites — the standard path (`src/plan_check.rs::run_impl`), the pre-decomposed extracted path (`src/plan_extract.rs` extracted), and the resume path (`src/plan_extract.rs` resume) — so the gate cannot be bypassed by routing through an alternative phase entry. Each violation in the JSON response carries a `rule` field (`"scope-enumeration"`, `"external-input-audit"`, or `"duplicate-test-coverage"`) tying it to the rule file (`.claude/rules/scope-enumeration.md`, `.claude/rules/external-input-audit-gate.md`, or `.claude/rules/duplicate-test-coverage.md`) the author should consult for the fix. Duplicate-test-coverage violations additionally carry `existing_test` and `existing_file` fields naming the pre-existing test's location. Contract tests in `tests/scope_enumeration.rs` and `tests/external_input_audit.rs` lock the committed prose corpus (CLAUDE.md, `.claude/rules/*.md`, `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) against drift for the scope-enumeration and external-input-audit rules. The duplicate-test-coverage scanner intentionally ships without a corpus contract test — legitimate educational test-name citations in rule files (e.g. `test_agent_frontmatter_only_supported_keys` above in the Sub-Agents section) would false-positive, and the Plan-phase gate already catches the real regression (a plan that names an existing test is rejected at plan-check time regardless of where the name appeared in committed prose). See `tests/duplicate_test_coverage.rs` for the rationale.

### Tombstone Lifecycle

Tombstone tests prevent merge conflicts from silently resurrecting deleted code. The lifecycle has two halves: creation (`.claude/rules/tombstone-tests.md`) and removal (`bin/flow tombstone-audit`). Standalone tombstones (file-existence, source-content checks) live in `tests/tombstones.rs`. Topical tombstones that are integral to a test domain (skill_contracts, structural, dispatcher) stay in their respective test files. The `tombstone-audit` subcommand scans ALL `tests/*.rs` files for PR references, queries GitHub for merge dates, and classifies each as stale or current. Code Review Step 1 runs the audit; Step 4 removes stale tombstones.

### 100% Coverage Is Enforced

Every production line in `src/*.rs` must be exercised by a named
test. The gate is `bin/test` itself — on full-suite runs it passes
`--fail-under-lines <L>`, `--fail-under-regions <R>`, and
`--fail-under-functions <F>` to `cargo llvm-cov nextest`. When any
aggregate falls below its threshold, CI exits non-zero and
`finalize-commit` blocks the commit.

The thresholds are pinned at 100/100/100. They are never lowered —
a regression is a CI-blocking failure, not grounds to relax the
gate.

The 100/100/100 gate is the load-bearing mechanism. `.claude/rules/no-waivers.md`
forbids per-line waiver files and the "measurement-only task"
antipattern that lets a session declare victory at <100%. The Code
Review reviewer agent treats any uncovered line as a Real finding
per `.claude/rules/code-review-scope.md`. Coverage-required tests —
tests whose only purpose is to exercise a branch — are explicitly
sanctioned by `.claude/rules/tests-guard-real-regressions.md`
"Coverage-Required Tests" because the gate itself names their
consumer.

## Test Architecture

All tests are Rust integration tests in `tests/*.rs`. Shared helpers in `tests/common/mod.rs` provide `repo_root()`, `bin_dir()`, `hooks_dir()`, `skills_dir()`, `docs_dir()`, `agents_dir()`, `load_phases()`, `load_hooks()`, `plugin_version()`, `phase_order()`, `utility_skills()`, `read_skill()`, `collect_md_files()`, and `create_git_repo_with_remote()`.

<!-- duplicate-test-coverage: not-a-new-test -->
Key test files: `tests/structural.rs` (config invariants, version consistency), `tests/skill_contracts.rs` (SKILL.md content via glob-based discovery — new skills auto-covered; `phase_skills_no_inline_time_computation` blocks skills that instruct Claude to compute values), `tests/permissions.rs` (allow/deny simulation, placeholder validation), `tests/docs_sync.rs` (docs completeness), `tests/concurrency.rs` (real-process concurrency).

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — clone QA repos, prime, run a full lifecycle, and verify results. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature. QA repos exist solely to test the FLOW lifecycle (Start through Complete) — `bin/flow ci` must run tests only, no linters or style checks. If `bin/flow ci` fails on seed code, fix the seed, don't debug the linter.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-changelog-audit` — `.claude/skills/flow-changelog-audit/SKILL.md` — audit Claude Code CHANGELOG.md for plugin-relevant changes, categorize as Adopt/Remove/Adapt, file issues

## Conventions

- **Never invoke `/flow-release` unless the user explicitly runs it** — fixing a bug does not authorize a release. Committing a fix and releasing it are separate decisions. The user decides when to ship.
- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts, no "just this once". The commit skill's `git diff --cached` check handles empty diffs via its "Nothing to commit" return path, so a session that only needs to verify CI never needs a shortcut. Infrastructure commits during `start-gate` (e.g., `commit_deps` for dependency lock files) are the sole carve-out: they commit directly via Rust under the start lock, before any worktree exists.
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
- **Extract-helper branch enumeration for refactor plans** — see `.claude/rules/extract-helper-refactor.md`
- **Duplicate test coverage for proposed test names** — see `.claude/rules/duplicate-test-coverage.md`
- **No `run_in_background` during FLOW phases**; `bin/flow` (any subcommand) is never allowed in the background regardless of mode — see `.claude/rules/ci-is-a-gate.md`. Enforced by `bin/flow hook validate-pretool`.
- **User evidence is ground truth** — when a user provides screenshots, error output, or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation. Never explain away evidence to preserve your analysis.
