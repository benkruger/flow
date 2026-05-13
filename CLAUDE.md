# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 5-phase development lifecycle: Start, Code, Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead. Language-agnostic — every project owns its toolchain via repo-local `bin/format`, `bin/lint`, `bin/build`, `bin/test` scripts that FLOW orchestrates.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and the four `bin/*` stubs as project config. `.flow.json` is git-excluded.
2. **As autonomous or manual as you want** — configurable via `.flow.json` skills settings.
3. **Safe for local env** — no containers, no permission prompts ever, native tools only.
4. **N×N×N concurrent** — N engineers running N flows on N boxes simultaneously is the primary use case. Local state (`.flow-states/`, worktrees) is per-machine; shared state (PRs, issues, labels) is coordinated through GitHub. Nothing assumes a single active flow.

After Complete, the only permanent artifacts are the merged PR and any CLAUDE.md learnings. Skills are pure Markdown instructions, not executable code. Tool dispatch is repo-local: `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` from cwd.

## The 5 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Create worktree, PR, state file, configure workspace; extract plan from issue body sentinels via `bin/flow plan-from-issue` |
| 2 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 3 | Review | `/flow:flow-review` | Six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation). Parent triages and fixes. |
| 4 | Learn | `/flow:flow-learn` | Capture learnings, route to permanent homes |
| 5 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

Phase gates enforced by `bin/flow check-phase` (`src/check_phase.rs`). Back-transitions defined in `flow-phases.json`.

Plan handoff happens at flow-start: `bin/flow plan-from-issue --issue <N> --branch <name>` fetches the issue body, extracts content between `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->` sentinels, and writes it to `.flow-states/<branch>/plan.md`. Issues filed via `/flow:flow-plan` and `/flow:flow-decompose-project` wrap their decompose output in those sentinels automatically.

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
- `agents/*.md` — ten custom plugin sub-agents split across two tiers. **Review tier (7):** ci-fixer, reviewer, pre-mortem, adversarial, learn-analyst, documentation, issue-triage. **Planning tier (3):** pm, tech-lead, cto.
- `src/*.rs` — Rust source for all `bin/flow` subcommands. Per-module purpose lives in module doc comments.
- `src/plan_from_issue.rs` — extracts plan content from issue-body sentinels at flow-start
- `src/validate_issue_body.rs` — pre-filing validator for issue bodies; reuses `plan_from_issue`'s sentinel constants, `extract_plan`, and `count_tasks` to reject bodies before `bin/flow issue`. Consumed by `flow-explore` (Filing step for vanilla problem statements), `flow-plan` (Filing step for decomposed plans), and `flow-decompose-project` (Step 3 epic + Step 4 per-child) so none of those skills can file an issue that `plan-from-issue` would later reject at flow-start
- `src/capture_diff.rs` — writes the full and substantive diffs against `origin/<base>` to canonical `.flow-states/<branch>/full-diff.diff` and `.flow-states/<branch>/substantive-diff.diff`. Consumed by `skills/flow-review/SKILL.md` Step 1 so the four Review sub-agents read the diff via file handoff (DIFF_FILE / SUBSTANTIVE_DIFF_FILE) instead of inline prompt bytes, keeping the parent skill's prompt budget bounded as PR size grows.
- `src/add_skipped_agent.rs` — appends `{agent, reason, timestamp}` to `phases.<phase>.agents_skipped` in state when flow-review's failure classification (Step 2) detects an agent returned an external-failure marker (`rate_limit`, `api_error`, `other`) or the retry-3-then-note loop exhausts (`exhausted_retries`). The `phase-finalize` `agents_skipped` gate reads the field and refuses to advance the phase unless `--accept-skipped-agents` is passed.
- `src/required_agents.rs` — authoritative per-phase `REQUIRED_AGENTS` constant binding each phase to its required-agent set (flow-review → reviewer/pre-mortem/adversarial/documentation; flow-learn → learn-analyst). Read by the `phase-finalize` required-agents gate and bound to the matching SKILL.md `subagent_type: "flow:<name>"` invocations by `tests/skill_contracts.rs::required_agents_matches_skill_invocations`. A SKILL.md edit that adds, removes, or renames an agent invocation without updating this constant fails the contract test.
- `src/record_agent_return.rs` — `bin/flow record-agent-return --branch <b> --agent <a> --phase <p>` records `{agent, timestamp}` to `phases.<phase>.agents_returned` AFTER verifying via the persisted Claude Code transcript that the agent's `Agent` tool_use + matching `tool_result` pair appears since the most recent `phase-enter --phase <phase>` Bash marker. The verifier (`crate::hooks::transcript_walker::verify_agent_returned_in_phase`) is the load-bearing protection against "model writes findings without invoking the agent": a model that did not actually invoke the agent cannot fabricate the state entry. Failure reasons: `transcript_path_invalid`, `phase_marker_not_found`, `tool_use_missing`, `tool_result_missing`.
- `src/commands/utility_marker.rs` — per-session `<home>/.claude/flow/utility-in-progress-<session_id>.json` marker that the Stop hook reads to refuse turn-end while a multi-step utility skill is running
- `src/session_metrics.rs` — token and rate-limit capture; reads `~/.claude/rate-limits.json` and the session transcript JSONL. Also owns the snapshot state mutators (`write_snapshot_into_state`, `append_step_snapshot`), session-id and transcript-path validators, and the `home_dir_or_empty` helper.
- `src/session_cost.rs` — per-session cost-file reads (`<project_root>/.claude/cost/<YYYY-MM>/<session_id>`) and monthly aggregation used by the TUI header.
- `src/per_flow_capture.rs` — orchestrator. Reads `session_id` and `transcript_path` from state, validates them, and bundles `session_metrics::capture` with `session_cost::read_cost_file` into a final `WindowSnapshot`.
- `bin/flow` — Rust dispatcher (auto-rebuilds when source is newer than binary)
- `bin/setup` — one-time install-flow script bundled with the plugin; checks for `cargo` and `cc` prerequisites (printing `brew install rust` / `xcode-select --install` hints when missing), then runs `cargo build --release` to compile the FLOW binary. Users invoke it from a plain terminal after `/plugin install` and before `/flow-prime`. Covered by `tests/bin_setup.rs` per the project's `tests/bin_<stem>.rs` convention.
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
- Stderr banner narrating CI rationale (caller-supplied via `--reason` or runner-inferred from sentinel state)

`bin/flow ci` always invokes the **worktree-root** scripts. For mono-repo flows started inside a service subdirectory, `ci::run_impl` normalizes cwd to the worktree root before scanning for `bin/<tool>` scripts. A repo with no `bin/{format,lint,build,test}` scripts is a hard error.

The four `bin/*` stubs are installed by `/flow:flow-prime` from `assets/bin-stubs/` when absent. Each stub carries a `# FLOW-STUB-UNCONFIGURED` marker; `bin/flow ci` refuses to write the sentinel when any tool is still a stub.

The `bin/test` stub additionally accepts `bin/test --adversarial-path` which prints the canonical Review adversarial probe test path. Exit 0 with single-line stdout = configured; exit 2 = unconfigured. The `EXCLUDE_ENTRIES` constant in `src/prime_check.rs` lists patterns prime adds to `.git/info/exclude` so the throwaway probe never appears in `git status`.

### Subdirectory Context

State files capture `relative_cwd` at flow-start time — the path inside the project root where `/flow:flow-start` was invoked. Empty string for root-level flows. For mono-repo flows started inside `api/`, `start-workspace` returns an absolute `worktree_cwd` that includes the suffix so the agent lands in `.worktrees/<branch>/api/`. `prime_check` reads `.flow.json` from the project root, so a mono-repo primed at the root passes prime-check from any app subdirectory.

Worktree creation mirrors every `.venv` discovered under the project root into the new worktree as a relative symlink (`src/start_workspace.rs::link_venvs`). The walker skips dotted directories other than `.venv`, a small named-skip list (`node_modules`, `target`, `vendor`, `build`, `dist`), and directory symlinks.

`cwd_scope::enforce` runs as the first action in every subcommand that runs tools or mutates state: `ci`, `build`, `lint`, `format`, `test`, `phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding`. Read-only subcommands (`format-status`, `status`, `tombstone-audit`, `base-branch`, `validate-issue-body`) do not enforce.

When a mono-repo session resumes (context compaction, orchestration, multi-skill chain), the agent's Bash tool cwd may reset to the main repo root and every subsequent `bin/flow` call hard-errors under `cwd_scope::enforce`. Two recovery paths exist: every `phase-enter` response carries a `worktree_cwd` field that joins `worktree_path` with `relative_cwd`, and the phase-enter skills (`flow-code`, `flow-review`, `flow-learn`) run `cd "<worktree_cwd>"` immediately after the HARD-GATE so the re-anchor is automatic at every phase entry. When `cwd_scope::enforce` still fires (e.g. mid-phase tool calls in a session that lost cwd between Bash invocations), the error message names the expected directory and ends with a copy-pasteable `cd "<expected>"` line the user can run verbatim.

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

Ten custom plugin sub-agents in `agents/*.md`, split across two tiers. Agent frontmatter must only use supported keys (`name`, `description`, `model`, `effort`, `maxTurns`, `tools`, `disallowedTools`, `skills`, `memory`, `background`, `isolation`) — `test_agent_frontmatter_only_supported_keys` enforces this. The global `PreToolUse` hook (`bin/flow hook validate-pretool`) enforces Bash and Agent tool restrictions across all agents. See `.claude/rules/cognitive-isolation.md`.

**Review tier (7):** ci-fixer (opus), reviewer (sonnet), pre-mortem (sonnet), adversarial (opus), learn-analyst (haiku), documentation (haiku), issue-triage (sonnet). Tiered by task complexity. Invoked by Review and Learn phase skills.

**Planning tier (3):** pm (haiku), tech-lead (sonnet), cto (opus). Designed for invocation by a future planning skill during design discussions. Scope authority escalates PM → Tech Lead → CTO via structured `## SCOPE REFUSAL` outputs; CTO is the escalation terminus and has no refusal block. The agents land first so the consuming skill can be authored against a stable contract.

When adding or modifying an agent's `maxTurns` budget, read peer agents' frontmatter to maintain parity.

**Agent return recording.** Required sub-agents (the four flow-review agents and flow-learn's learn-analyst — see `src/required_agents.rs::REQUIRED_AGENTS`) participate in a transcript-verified recording contract. After a sub-agent returns cleanly (Class 3 in flow-review Step 2 / flow-learn Step 1), the calling skill invokes `bin/flow record-agent-return --branch <b> --agent <name> --phase <p>` which verifies the `Agent` tool_use + matching `tool_result` pair appears in the persisted transcript since the most recent `phase-enter --phase <p>` marker, then appends `{agent, timestamp}` to `phases.<phase>.agents_returned`. The `phase-finalize` required-agents gate refuses to advance the phase when any required agent appears in neither `agents_returned` nor `agents_skipped`. This closes the "model writes findings without invoking the agent" bypass.

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

All 5 phases produce log entries. Most use `[Phase N] module — step (status)` format. N is derived from `phase_number()` in `phase_config.rs`. `finalize_commit.rs` reads `current_phase` from the state file. Phase 5 modules use guarded logging to avoid creating `.flow-states/` in test fixtures.

### Version Locations

Version lives in 3 places (across 2 files), all must match: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` top-level, `.claude-plugin/marketplace.json` plugins array. `tests/structural.rs` enforces consistency.

### Checksum → Version Invariant

`config_hash` covers permission structure (allow/deny lists, defaultMode, exclude entries). `setup_hash` is a SHA-256 of `src/prime_setup.rs`. Both stored in `.flow.json`, compared by `prime_check.rs` on version mismatch. Matching hashes allow auto-upgrade; mismatching hashes force a full `/flow:flow-prime` re-run.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands:

- `phase-enter` — phase entry (gate check + enter + step counters + state data return)
- `phase-finalize` — phase completion (complete + Slack + notification record)
- `phase-transition` — Complete-phase transition path
- `set-timestamp` — mid-phase fields
- `add-finding` — recording triage findings to `findings[]`
- `add-skipped-agent` — recording skipped review agents to `phases.<phase>.agents_skipped` (reasons: `rate_limit`, `api_error`, `other`, `exhausted_retries`); read by the `phase-finalize` `agents_skipped` gate
- `record-agent-return` — recording verified agent returns to `phases.<phase>.agents_returned` (transcript-verified before write); read by the `phase-finalize` required-agents gate

`code_task` can only be incremented by 1 per `--set` argument — `apply_updates` validates each `--set code_task=N` sequentially. Batch counter advances in one call for atomic commit groups.

Plan file: `.flow-states/<branch>/plan.md`, stored in `state["files"]["plan"]`. The plan is extracted from the GitHub issue body at flow-start by `bin/flow plan-from-issue` (looks for `<!-- FLOW-PLAN-BEGIN -->`/`<!-- FLOW-PLAN-END -->` sentinels). Legacy state files may still use top-level `state["plan_file"]`.

Account-window snapshots are captured at every state-mutating transition by `src/per_flow_capture.rs::capture_for_active_state` — at flow start, every phase enter/complete, every step counter increment, and flow complete. The orchestrator bundles `session_metrics::capture` (rate limits + transcript tokens) with `session_cost::read_cost_file` (per-session cost). Each snapshot records account-window pcts (5h, 7d), session token totals with per-model split, session cost, turn/tool counts, and most-recent-turn context utilization. Every numeric field is `Option<...>` for fail-open semantics. Consumers read snapshots through `src/window_deltas.rs` which groups by `session_id`.

### Start-Init → Init-State Contract

`start-init` derives the canonical branch name (issue-aware via `fetch_issue_info` + `branch_name`) BEFORE acquiring the start lock. It computes `relative_cwd` from `cwd.canonicalize().strip_prefix(project_root.canonicalize())`. It then passes `--branch <canonical> --relative-cwd <rel>` to the `init-state` subprocess, which uses the provided values directly. This ensures the lock is acquired and released under the same name.

### Auto-Advance Architecture

Three layers:

1. The phase completion command returns `continue_action` (`"invoke"` or `"ask"`) and optionally `continue_target` in its JSON. Skill HARD-GATEs parse `continue_action` to decide whether to auto-invoke the next phase or prompt the user.
2. `phase_complete()` writes `_auto_continue` to the state file when `continue_action` is `"invoke"`. The `validate-ask-user` PreToolUse hook reads this and auto-answers any `AskUserQuestion` that fires — safety net for cases where the model ignores the HARD-GATE.
3. **Autonomous Stop Enforcement.** The Stop hook (`stop_continue::run()`) runs six predicates in order: `check_halt_pending` (explicit-user-pause detection — sets/clears `_halt_pending` based on the user's most recent prose message after the model's latest Skill action, runs FIRST so a "pause" directive cannot be misclassified as discussion mode), `check_first_stop` (discussion mode + first-stop pending), `check_continue` (multi-child-skill chains), `check_in_progress_utility_skill` (multi-step utility skills like `flow:flow-decompose-project` and `flow:flow-plan` with a per-session marker at `<home>/.claude/flow/utility-in-progress-<id>.json`), `check_prose_pause_at_task_entry` (Code-phase task-entry boundary with a prose question and no tool call), and `check_autonomous_in_progress` (generic in-progress + auto + empty `_continue_pending` block). The last three close the text-only-stop hole that PreToolUse hooks cannot reach: PreToolUse fires on tool calls, so a model that ends the turn with prose alone is invisible to it; the Stop hook fires on the Stop event itself. The halt-pause predicate is composed BEFORE `check_first_stop` so the conversation-preserving block message (naming the closed continue-token grammar — `continue`, `resume`, `proceed`, `go ahead`, `keep going`) dominates the generic discussion-mode message. The utility-skill predicate is composed BEFORE the prose-pause predicate so its marker-specific block message wins when both could fire; the prose-pause predicate is composed BEFORE the generic autonomous block so its more specific message (citing `.claude/rules/autonomous-flow-self-recovery.md`) wins for the prose-pause shape; other text-only stops fall through to the generic predicate. See `.claude/rules/autonomous-phase-discipline.md` Enforcement section, "Mechanical halt-pause contract" subsection, and "Prose-Based Pauses Bypass AskUserQuestion" subsection.

Block-first ordering: when the current phase's `phases.<current_phase>.status == "in_progress"` AND `skills.<current_phase>.continue == "auto"`, `validate-ask-user` returns exit 2 instead of auto-answering. The block path precedes the auto-answer path. The `in_progress` scope is load-bearing: the next phase's status is still `"pending"` until `phase_enter()` runs, so the completing skill's HARD-GATE prompt to approve transitions is NOT blocked even when the next phase is auto. `phase_enter()` clears `_auto_continue`, `_continue_pending`, `_continue_context`. See `.claude/rules/autonomous-phase-discipline.md`.

### Permission Invariant

Every bash block in every skill must run without triggering a permission prompt. `tests/permissions.rs` enforces at test time; `bin/flow hook validate-pretool` enforces at runtime via global PreToolUse hook (compound commands, command substitution, redirection blocked; whitelist enforced when a flow is active; `general-purpose` sub-agents blocked during active phases).

Layer 9 mechanically blocks direct commit invocations (`git ... commit`, `bin/flow ... finalize-commit`) when the effective cwd resolves to the integration branch OR to a feature branch with an active state file. Each context carries its own carve-out:

- **Active-flow context** — `bin/flow ... finalize-commit` (only that shape, never `git commit`) passes when the state file has `_continue_pending == "commit"` AND the persisted transcript shows the most recent assistant Skill is one of `flow:flow-commit` or `flow-release` (the shared two-arm `transcript_shows_commit_window_skill` predicate; in practice every active-flow commit names `flow:flow-commit` because the release path runs on the integration trunk, not under an active flow).
- **Integration-branch context** — `bin/flow ... finalize-commit` (only that shape) passes when the persisted transcript shows BOTH the most recent assistant Skill is one of `flow:flow-commit` (delegated commit path) or `flow-release` (direct commit path) AND a sanctioned bootstrap parent (`flow:flow-start`, `flow:flow-prime`, or `flow-release`) in the post-user-turn chain. The integration-branch context has no per-branch state file, so the second walker condition substitutes for the marker. `flow-release` is the bare-name project-local maintainer skill at `.claude/skills/flow-release/`; the other two bootstrap parents are plugin-marketplace skills at `skills/<name>/` and carry the `flow:` prefix in their emission.

Raw `git commit` is never carved out in either context.

`validate-ask-user` blocks `AskUserQuestion` calls with exit 2 when the current phase is both in-progress AND autonomous.

See `.claude/rules/concurrency-model.md` "Mechanical Enforcement" and `.claude/rules/permissions.md`.

### User-Only Skill Enforcement

Four FLOW skills are reserved for direct user invocation: `/flow:flow-abort`, `/flow:flow-reset`, `/flow-release`, and `/flow:flow-prime`. The model must never invoke them. Three independent mechanical layers enforce this:

1. **Layer 1 — `validate-skill` (PreToolUse:Skill)**. `src/hooks/validate_skill.rs` blocks Skill tool calls naming a user-only skill unless the persisted transcript at `transcript_path` shows the most recent user-role turn typed `<command-name>/<skill></command-name>`. Backed by `src/hooks/transcript_walker.rs::last_user_message_invokes_skill`.
2. **Layer 2 — `validate-ask-user` carve-out**. `src/hooks/validate_ask_user.rs::user_only_skill_carve_out_applies` allows `AskUserQuestion` to fire even during in-progress autonomous phases when the most recent assistant Skill tool_use call (since the most recent user turn) targets a user-only skill. Resolves the abort-during-autonomous-flow deadlock.
3. **Layer 3 — `validate-claude-paths` transcript root lockdown**. `src/hooks/validate_claude_paths.rs::is_transcript_path` blocks Edit/Write on `~/.claude/projects/` regardless of flow state. Tampering with the persisted transcript would subvert Layer 1's user-invocation check.

The transcript walker (`src/hooks/transcript_walker.rs`) is shared infrastructure between Layer 1 and Layer 2. `USER_ONLY_SKILLS` is the authoritative list. Reads are capped at `TRANSCRIPT_BYTE_CAP` (50 MB) per `.claude/rules/external-input-path-construction.md`. Walkers discriminate real user turns from synthetic ones (tool_result wrappers, hook-injected feedback with `isMeta:true`) via `is_real_user_turn`. See `.claude/rules/user-only-skills.md` for the full design and the per-skill threat-shape rationale, and `.claude/rules/transcript-shape.md` for the synthetic-turn catalog and the mechanical contract walkers must satisfy.

### Tombstone Lifecycle

Tombstone tests prevent merge conflicts from silently resurrecting deleted code. Standalone tombstones live in `tests/tombstones.rs`; topical tombstones integral to a test domain stay in their respective test files. `bin/flow tombstone-audit` scans all `tests/*.rs` for PR references, queries GitHub for merge dates, and classifies each as stale or current. Review Step 1 runs the audit; Step 4 removes stale tombstones. See `.claude/rules/tombstone-tests.md`.

### 100% Coverage Is Enforced

Every production line in `src/*.rs` must be exercised by a named test. The gate is `bin/test` itself — full-suite runs pass `--fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100` to `cargo llvm-cov nextest`. When any aggregate falls below threshold, CI exits non-zero and `finalize-commit` blocks the commit.

Thresholds are pinned at 100/100/100 — never lowered. `.claude/rules/no-waivers.md` forbids per-line waiver files. Coverage-required tests are sanctioned by `.claude/rules/tests-guard-real-regressions.md`.

## Test Architecture

All tests are Rust integration tests in `tests/*.rs`. Shared helpers in `tests/common/mod.rs`: `repo_root()`, `bin_dir()`, `hooks_dir()`, `skills_dir()`, `docs_dir()`, `agents_dir()`, `load_phases()`, `load_hooks()`, `plugin_version()`, `phase_order()`, `utility_skills()`, `read_skill()`, `collect_md_files()`, `create_git_repo_with_remote()`.

Key test files: `tests/structural.rs` (config invariants, version consistency), `tests/skill_contracts.rs` (SKILL.md content via glob-based discovery — `phase_skills_no_inline_time_computation` blocks skills that instruct Claude to compute values), `tests/permissions.rs`, `tests/docs_sync.rs`, `tests/concurrency.rs`.

## Maintainer Skills (private to this repo)

- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/flow-changelog-audit` — audit Claude Code CHANGELOG.md for plugin-relevant changes

When developing FLOW itself, point Claude Code at the local plugin source via `claude --plugin-dir=$HOME/code/flow`. The installed marketplace plugin enforces phase counts and skill gates from the released version, which conflict with in-progress source changes; `--plugin-dir` overrides for the session.

## Conventions

- **Commit discipline** — see `.claude/rules/concurrency-model.md`.
- **CI is a gate** — see `.claude/rules/ci-is-a-gate.md` and `.claude/rules/always-verify.md`.
- New skills are automatically covered by `tests/skill_contracts.rs`.
- Namespace is `flow:` — plugin.json name is `"flow"`.
- Never rebase — branch protection requires merge-only.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments. All computation goes through `bin/flow` subcommands.
- **All timestamps use Pacific Time** — `src/utils.rs::now()` returns Pacific Time ISO 8601. All Rust code uses this function.
- **Prefer dedicated tools over Bash** — see `.claude/rules/worktree-commands.md`.
- **Issue filing** — see `.claude/rules/filing-issues.md`.
- **Repo-level targets only** — see `.claude/rules/repo-level-only.md`.
- **Extract-helper branch enumeration for refactor plans** — see `.claude/rules/extract-helper-refactor.md`.
- **Deletion-sweep evidence for delete/rename proposals** — see `.claude/rules/docs-with-behavior.md` "Scope Enumeration (Rename Side)".
- **Tombstone five-item checklist for tombstone proposals** — see `.claude/rules/tombstone-tests.md` "Plan-phase responsibility".
- **Verify cited identifiers exist as `fn` definitions** — see `.claude/rules/skill-authoring.md` "Verify Test Function References in Issues".
- **Ephemeral worktree-internal artifact cleanup** — disposed before `git worktree remove` via `fs::remove_file` for permission-safe, audit-trailed removal — see `.claude/rules/ephemeral-file-cleanup.md`.
- **No run_in_background for bin/flow** — see `.claude/rules/ci-is-a-gate.md`.
- **User-only skills (model must never invoke)** — see `.claude/rules/user-only-skills.md`. The model must not invoke `/flow:flow-abort`, `/flow:flow-reset`, `/flow-release`, or `/flow:flow-prime`; the user types these directly.
- **No backwards-reasoning** — see `.claude/rules/no-backwards-reasoning.md`. Decisions about current code stand on current merits, not on commit messages, PR descriptions, doc comments, `git log`, or `git blame` as authority. Issue-filing skills (`flow-plan`, `flow-decompose-project`) include a mechanical scan that fires before the draft is presented.
- **Include bias in issues** — see `.claude/rules/include-bias-in-issues.md`. Default to including adjacent concerns in an issue's scope; valid exclusions name a concrete blocker, not a defensive enumeration. Issue-filing skills (`flow-plan`, `flow-decompose-project`) include a mechanical scan for defensive-scope phrasings ("Out of scope", "Non-goals", "would expand scope", "separate code surface") that fires before the draft is presented.
- **User evidence is ground truth** — when a user provides screenshots or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation.
- **Transcript walker real-vs-synthetic discrimination** — see `.claude/rules/transcript-shape.md`. Backward walkers over `~/.claude/projects/.../transcript.jsonl` must call `is_real_user_turn` (or apply the targeted hook-feedback skip) so synthetic user turns — tool_result wrappers AND hook-injected feedback turns carrying `isMeta:true` — never halt the walker on the way to a real user message. Inlining the discrimination is forbidden.
