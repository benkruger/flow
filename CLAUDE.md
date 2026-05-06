# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 6-phase development lifecycle: Start, Plan, Code, Code Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead. Language-agnostic — every project owns its toolchain via repo-local `bin/format`, `bin/lint`, `bin/build`, `bin/test` scripts that FLOW orchestrates.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and the four `bin/*` stubs as project config. `.flow.json` is git-excluded.
2. **As autonomous or manual as you want** — configurable via `.flow.json` skills settings.
3. **Safe for local env** — no containers, no permission prompts ever, native tools only.
4. **N×N×N concurrent** — N engineers running N flows on N boxes simultaneously is the primary use case. Local state (`.flow-states/`, worktrees) is per-machine; shared state (PRs, issues, labels) is coordinated through GitHub. Nothing assumes a single active flow.

After Complete, the only permanent artifacts are the merged PR and any CLAUDE.md learnings. Skills are pure Markdown instructions, not executable code. Tool dispatch is repo-local: `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` from cwd.

## The 6 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Create worktree, PR, state file, configure workspace |
| 2 | Plan | `/flow:flow-plan` | Invoke decompose plugin, explore codebase, create implementation plan |
| 3 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 4 | Code Review | `/flow:flow-code-review` | Six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation). Parent triages and fixes. |
| 5 | Learn | `/flow:flow-learn` | Capture learnings, route to permanent homes |
| 6 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

Phase gates enforced by `bin/flow check-phase` (`src/check_phase.rs`). Back-transitions defined in `flow-phases.json`.

## When You Must Update Docs and Tests

"Marketing docs" refers to `docs/index.html` — the GitHub Pages landing page.

### Structural sync (CI-enforced by `tests/docs_sync.rs`)

- New/renamed skill — `docs/skills/<name>.md`, `docs/skills/index.md`, `README.md`
- New/renamed phase — `docs/phases/phase-<N>-<name>.md`, `docs/skills/index.md`, `README.md`, `docs/index.html`
- New feature/capability — `README.md` and `docs/index.html` must mention required keywords (see `required_features()` in `tests/docs_sync.rs`)

### Content sync (convention-enforced)

- Changed skill behavior → `docs/skills/<name>.md` and Description column in `docs/skills/index.md`
- Changed phase behavior → `docs/phases/phase-<N>-<name>.md` and `docs/skills/index.md`
- Changed architecture → `README.md` and `docs/index.html`

### Test requirements

- New skills auto-covered by `tests/skill_contracts.rs` (glob-based discovery)
- Any new executable code needs tests — skills are Markdown and don't need tests beyond contracts

## Key Files

- `config.json` — plugin-level maintainer config (`claude_code_audited` tracks last audited Claude Code version)
- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's Markdown instructions
- `hooks/hooks.json` — hook registration
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site; `docs/reference/flow-state-schema.md` for state file schema
- `agents/*.md` — six custom plugin sub-agents (ci-fixer, reviewer, pre-mortem, adversarial, learn-analyst, documentation)
- `src/*.rs` — Rust source for all `bin/flow` subcommands. Per-module purpose lives in module doc comments.
- `bin/flow` — Rust dispatcher (auto-rebuilds when source is newer than binary)
- `bin/{format,lint,build,test}` — FLOW's own dogfood scripts
- `assets/bin-stubs/` — self-documenting bash stubs that prime copies into target projects when absent
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)

## Development Environment

- Run tests with `bin/flow ci` only — never invoke cargo directly
- `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` in sequence (format first for fail-fast). In THIS repo, `bin/build` is a no-op — compilation happens inside `bin/test` via `cargo-llvm-cov nextest`.
- `bin/flow ci --format`/`--lint`/`--build`/`--test` runs only that single phase. Single-phase runs disable both sentinel read and write.
- `bin/flow ci --force` runs all four AND bypasses the sentinel skip.
- `bin/flow ci --clean` is the user-facing deep-reset (wipes sentinel, profraws, `target/llvm-cov-target/debug/`).
- **For single-file coverage iteration, use `bin/test tests/<name>.rs`** — runs only that test binary and asserts 100/100/100 against the mirrored `src/<name>.rs`. Seconds vs ~3 minutes for full CI. See `.claude/rules/per-file-coverage-iteration.md`.
- **Use `bin/flow ci --test -- <filter>` for targeted test runs across the workspace.**
- **`bin/test` sweeps `*.profraw` recursively under `target/llvm-cov-target/` at the start of every invocation** to keep coverage measurement scoped to the current run.
- **`bin/test --show <file>`** renders annotated source coverage. **`bin/test --funcs <file>`** lists every function instantiation with its execution count (used to confirm phantom-miss diagnosis from stale instrumented binaries).
- Dependencies managed via `bin/dependencies` (runs `cargo update`).

## Architecture

### Plugin vs Target Project

Skills and hooks run in the target project's working directory, not the plugin source. State files live in the target project's `.flow-states/`. Hooks must be tested against a target project layout, not this repo.

### Skills Are Markdown, Not Code

Skills are pure Markdown (`skills/<name>/SKILL.md`). The only executable code is `bin/flow` (dispatcher) and `src/*.rs` (Rust source).

### Repo-Local Tool Delegation

`bin/flow ci` (and `--format`/`--lint`/`--build`/`--test`) spawns `./bin/<tool>` from cwd. The user's `bin/<tool>` script owns the actual command. FLOW contributes:

- Sentinel-based dirty-check (`tree_snapshot` SHA-256 over HEAD + diff + untracked)
- Retry/flaky classification (test only)
- `FLOW_CI_RUNNING=1` recursion guard
- Fail-fast tool ordering (format → lint → build → test)
- Stable JSON output contract
- Cwd-drift guard via `cwd_scope::enforce`

`bin/flow ci` always invokes the **worktree-root** scripts. For mono-repo flows started inside a service subdirectory, `ci::run_impl` normalizes cwd to the worktree root before scanning for `bin/<tool>` scripts. A repo with no `bin/{format,lint,build,test}` scripts is a hard error.

The four `bin/*` stubs are installed by `/flow:flow-prime` from `assets/bin-stubs/` when absent. Each stub carries a `# FLOW-STUB-UNCONFIGURED` marker; `bin/flow ci` refuses to write the sentinel when any tool is still a stub.

The `bin/test` stub additionally accepts `bin/test --adversarial-path` which prints the canonical Code Review adversarial probe test path. Exit 0 with single-line stdout = configured; exit 2 = unconfigured. The `EXCLUDE_ENTRIES` constant in `src/prime_check.rs` lists patterns prime adds to `.git/info/exclude` so the throwaway probe never appears in `git status`.

### Subdirectory Context

State files capture `relative_cwd` at flow-start time — the path inside the project root where `/flow:flow-start` was invoked. Empty string for root-level flows. For mono-repo flows started inside `api/`, `start-workspace` returns an absolute `worktree_cwd` that includes the suffix so the agent lands in `.worktrees/<branch>/api/`. `prime_check` reads `.flow.json` from the project root, so a mono-repo primed at the root passes prime-check from any app subdirectory.

Worktree creation mirrors every `.venv` discovered under the project root into the new worktree as a relative symlink (`src/start_workspace.rs::link_venvs`). The walker skips dotted directories other than `.venv`, a small named-skip list (`node_modules`, `target`, `vendor`, `build`, `dist`), and directory symlinks.

`cwd_scope::enforce` runs as the first action in every subcommand that runs tools or mutates state: `ci`, `build`, `lint`, `format`, `test`, `phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding`. Read-only subcommands (`format-status`, `tombstone-audit`, `plan-check`, `base-branch`) do not enforce.

### State File

The state file (`.flow-states/<branch>/state.json`) is the backbone. Schema reference: `docs/reference/flow-state-schema.md`. Test fixtures: `tests/common/mod.rs` helpers.

### Local vs Shared State

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API |

The "Flow In-Progress" label on issues is the cross-engineer WIP detection mechanism. See `.claude/rules/concurrency-model.md`.

### Start-Gate CI on the Base Branch as Serialization Point

`start-gate` runs `bin/flow ci` on the integration branch (`base_branch` captured at flow-start by `init_state` from `git symbolic-ref --short refs/remotes/origin/HEAD`), under the start lock. This is the coordination surface for dependency-maintenance work across all concurrent flows: the first flow-start of the day acquires the lock, runs CI on the base branch, and if a dependency upgrade broke something, `ci-fixer` repairs it once. Subsequent flow-starts queue behind the lock; when they acquire, the CI sentinel (`.flow-states/<base_branch>/ci-passed`) lets them pass through without re-running CI. Dependency churn costs O(1), not O(N).

The base-branch sentinel is also written by `complete-finalize` at the end of every flow that pulled cleanly, fired only when `--pull` was passed AND `cleanup_map["git_pull"] == "pulled"`.

The base branch's `target/` is a long-lived build surface across many source generations. Tools that write artifacts there must stay coherent — `bin/test`'s profraw sweep is the mechanism.

### Sub-Agents

<!-- duplicate-test-coverage: not-a-new-test -->
Six custom plugin sub-agents in `agents/*.md` — tiered by task complexity: opus (ci-fixer, adversarial), sonnet (reviewer, pre-mortem), haiku (learn-analyst, documentation). Agent frontmatter must only use supported keys (`name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`) — `test_agent_frontmatter_only_supported_keys` enforces this. The global `PreToolUse` hook (`bin/flow hook validate-pretool`) enforces Bash and Agent tool restrictions across all agents. See `.claude/rules/cognitive-isolation.md`.

When adding or modifying an agent's `maxTurns` budget, read peer agents' frontmatter to maintain parity.

### Orchestration

`/flow:flow-orchestrate` processes decomposed issues overnight. Fetches open issues labeled "Decomposed", filters out "Flow In-Progress", runs each sequentially via `flow-start --auto`. State tracked in `.flow-states/orchestrate.json` (machine-level singleton). Only one orchestration runs per machine at a time.

### Memory and Learning System

Auto-memory is shared across git worktrees of the same repository (since Claude Code 2.1.63).

Learn routes learnings to project CLAUDE.md and `.claude/rules/`. Files GitHub issues for process gaps via `bin/flow add-issue`. Records triage findings via `bin/flow add-finding`.

### Commit Path Gates

CI is enforced inside `finalize-commit` itself — `run_impl` calls `ci::run_impl()` before `git commit`, so every commit path runs CI mechanically. The `commit_format` preference is copied from `.flow.json` into the state file by `/flow-start`. After `finalize-commit` succeeds and `git pull` did not introduce new content, the CI sentinel auto-refreshes.

Three gates run inside `finalize_commit::run_impl` before `git commit`:

1. **Working-tree-dirty gate** — `git diff --quiet` checks whether the working tree differs from the index. When it does, returns `{"status":"error","step":"working_tree_dirty"}`. CI tools read the working tree but `git commit -F` commits the index — when they diverge, CI tests one set of bytes and the commit lands a different set. Refuse-not-resolve: user must `git add` (commit) or `git restore` (drop).
2. **CI gate** — `ci::run_impl()` runs the four-tool dispatch.
3. **Plan deviation gate** — `crate::plan_deviation::run_impl` reads the plan's `## Tasks` section, collects `(test_name, fixture_key, plan_value)` triples, and cross-references each plan value against string literals in the diff-added test function's body. Drift not acknowledged by a matching `bin/flow log` entry blocks with `{"status":"error","step":"plan_deviation"}`. See `.claude/rules/plan-commit-atomicity.md`.

### Logging

Phase skills log completion events to `.flow-states/<branch>/log` using a command-first pattern. Logging goes to `.flow-states/`, never `/tmp/`.

All 6 phases produce log entries. Most use `[Phase N] module — step (status)` format. N is derived from `phase_number()` in `phase_config.rs`. `finalize_commit.rs` reads `current_phase` from the state file. Phase 6 modules use guarded logging to avoid creating `.flow-states/` in test fixtures.

### Version Locations

Version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` top-level, `.claude-plugin/marketplace.json` plugins array. `tests/structural.rs` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of `src/prime_setup.rs`. Both stored in `.flow.json`, compared by `prime_check.rs` on version mismatch. Matching hashes allow auto-upgrade; mismatching hashes force a full `/flow:flow-prime` re-run.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands:

- `phase-enter` — phase entry (gate check + enter + step counters + state data return)
- `phase-finalize` — phase completion (complete + Slack + notification record)
- `phase-transition` — phases not yet migrated (Plan entry, Complete)
- `set-timestamp` — mid-phase fields
- `add-finding` — recording triage findings to `findings[]`

Exception: `plan-extract` writes Plan phase step fields directly via `mutate_state` when handling the extracted path.

`code_task` can only be incremented by 1 per `--set` argument — `apply_updates` validates each `--set code_task=N` sequentially. Batch counter advances in one call for atomic commit groups.

Plan file: `.flow-states/<branch>/plan.md`, stored in `state["files"]["plan"]`. DAG file: `.flow-states/<branch>/dag.md`, stored in `state["files"]["dag"]`. Legacy state files may still use top-level `state["plan_file"]` and `state["dag_file"]`.

Account-window snapshots are captured at every state-mutating transition by `src/window_snapshot.rs::capture` — at flow start, every phase enter/complete, every step counter increment, and flow complete. Each snapshot records account-window pcts (5h, 7d), session token totals with per-model split, session cost, turn/tool counts, and most-recent-turn context utilization. Every numeric field is `Option<...>` for fail-open semantics. Consumers read snapshots through `src/window_deltas.rs` which groups by `session_id`.

### Start-Init → Init-State Contract

`start-init` derives the canonical branch name (issue-aware via `fetch_issue_info` + `branch_name`) BEFORE acquiring the start lock. It computes `relative_cwd` from `cwd.canonicalize().strip_prefix(project_root.canonicalize())`. It then passes `--branch <canonical> --relative-cwd <rel>` to the `init-state` subprocess, which uses the provided values directly. This ensures the lock is acquired and released under the same name.

### Auto-Advance Architecture

Two layers:

1. The phase completion command returns `continue_action` (`"invoke"` or `"ask"`) and optionally `continue_target` in its JSON. Skill HARD-GATEs parse `continue_action` to decide whether to auto-invoke the next phase or prompt the user.
2. `phase_complete()` writes `_auto_continue` to the state file when `continue_action` is `"invoke"`. The `validate-ask-user` PreToolUse hook reads this and auto-answers any `AskUserQuestion` that fires — safety net for cases where the model ignores the HARD-GATE.

Block-first ordering: when the current phase's `phases.<current_phase>.status == "in_progress"` AND `skills.<current_phase>.continue == "auto"`, `validate-ask-user` returns exit 2 instead of auto-answering. The block path precedes the auto-answer path. The `in_progress` scope is load-bearing: the next phase's status is still `"pending"` until `phase_enter()` runs, so the completing skill's HARD-GATE prompt to approve transitions is NOT blocked even when the next phase is auto. `phase_enter()` clears `_auto_continue`, `_continue_pending`, `_continue_context`. See `.claude/rules/autonomous-phase-discipline.md`.

### Permission Invariant

Every bash block in every skill must run without triggering a permission prompt. `tests/permissions.rs` enforces at test time; `bin/flow hook validate-pretool` enforces at runtime via global PreToolUse hook (compound commands, command substitution, redirection, file-read commands blocked; whitelist enforced when a flow is active; `general-purpose` sub-agents blocked during active phases).

Layer 10 mechanically blocks direct commit invocations (`git ... commit`, `bin/flow ... finalize-commit`) when the effective cwd resolves to the integration branch OR to a feature branch with an active state file. The active-flow context carries a skill-commit carve-out: `bin/flow ... finalize-commit` (only that shape, never `git commit`) passes through when the state file has `_continue_pending == "commit"`. The integration-branch context is NOT carved out.

`validate-ask-user` blocks `AskUserQuestion` calls with exit 2 when the current phase is both in-progress AND autonomous.

See `.claude/rules/concurrency-model.md` "Mechanical Enforcement" and `.claude/rules/permissions.md`.

### Plan-Phase Gates

Phase 2 gates completion on three scanners that share `bin/flow plan-check`:

- `src/scope_enumeration.rs::scan` — universal-coverage prose without a named sibling list
- `src/external_input_audit.rs::scan` — panic/assert tightening proposals without a paired callsite source-classification audit table
- `src/duplicate_test_coverage.rs::scan` — proposed test names that normalize to an existing test in `tests/**/*.rs`

All three run at three callsites: standard path (`src/plan_check.rs`), pre-decomposed extracted path, and resume path (both in `src/plan_extract.rs`). Each violation carries a `rule` field tying it to its rule file. Contract tests in `tests/scope_enumeration.rs` and `tests/external_input_audit.rs` lock the committed prose corpus against drift.

### Tombstone Lifecycle

Tombstone tests prevent merge conflicts from silently resurrecting deleted code. Standalone tombstones live in `tests/tombstones.rs`; topical tombstones integral to a test domain stay in their respective test files. `bin/flow tombstone-audit` scans all `tests/*.rs` for PR references, queries GitHub for merge dates, and classifies each as stale or current. Code Review Step 1 runs the audit; Step 4 removes stale tombstones. See `.claude/rules/tombstone-tests.md`.

### 100% Coverage Is Enforced

Every production line in `src/*.rs` must be exercised by a named test. The gate is `bin/test` itself — full-suite runs pass `--fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100` to `cargo llvm-cov nextest`. When any aggregate falls below threshold, CI exits non-zero and `finalize-commit` blocks the commit.

Thresholds are pinned at 100/100/100 — never lowered. `.claude/rules/no-waivers.md` forbids per-line waiver files. Coverage-required tests are sanctioned by `.claude/rules/tests-guard-real-regressions.md`.

## Test Architecture

All tests are Rust integration tests in `tests/*.rs`. Shared helpers in `tests/common/mod.rs`: `repo_root()`, `bin_dir()`, `hooks_dir()`, `skills_dir()`, `docs_dir()`, `agents_dir()`, `load_phases()`, `load_hooks()`, `plugin_version()`, `phase_order()`, `utility_skills()`, `read_skill()`, `collect_md_files()`, `create_git_repo_with_remote()`.

<!-- duplicate-test-coverage: not-a-new-test -->
Key test files: `tests/structural.rs` (config invariants, version consistency), `tests/skill_contracts.rs` (SKILL.md content via glob-based discovery — `phase_skills_no_inline_time_computation` blocks skills that instruct Claude to compute values), `tests/permissions.rs`, `tests/docs_sync.rs`, `tests/concurrency.rs`.

## Maintainer Skills (private to this repo)

- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-changelog-audit` — audit Claude Code CHANGELOG.md for plugin-relevant changes

When developing FLOW itself, point Claude Code at the local plugin source via `claude --plugin-dir=$HOME/code/flow`. The installed marketplace plugin enforces phase counts and skill gates from the released version, which conflict with in-progress source changes; `--plugin-dir` overrides for the session.

## Conventions

- **Never invoke `/flow-release` unless the user explicitly runs it** — fixing a bug does not authorize a release.
- All commits via `/flow:flow-commit` skill — no exceptions, no shortcuts. Infrastructure commits during `start-gate` (e.g., `commit_deps` for dependency lock files) are the sole carve-out: they commit directly via Rust under the start lock, before any worktree exists.
- All changes require `bin/flow ci` green before committing — tests are the gate.
- New skills are automatically covered by `tests/skill_contracts.rs`.
- Namespace is `flow:` — plugin.json name is `"flow"`.
- Never rebase — merge only.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments. All computation goes through `bin/flow` subcommands.
- **All timestamps use Pacific Time** — `src/utils.rs::now()` returns Pacific Time ISO 8601. All Rust code uses this function.
- **Prefer dedicated tools over Bash** — see `.claude/rules/worktree-commands.md`.
- **Issue filing** — see `.claude/rules/filing-issues.md`.
- **Repo-level targets only** — see `.claude/rules/repo-level-only.md`.
- **Scope enumeration for universal-coverage claims** — see `.claude/rules/scope-enumeration.md`.
- **External-input audit for panic/assert tightenings** — see `.claude/rules/external-input-audit-gate.md`.
- **Extract-helper branch enumeration for refactor plans** — see `.claude/rules/extract-helper-refactor.md`.
- **Duplicate test coverage for proposed test names** — see `.claude/rules/duplicate-test-coverage.md`.
- **No `run_in_background` during FLOW phases**; `bin/flow` is never allowed in the background — see `.claude/rules/ci-is-a-gate.md`.
- **User evidence is ground truth** — when a user provides screenshots or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation.
