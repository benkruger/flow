# CLAUDE.md

## You Don't Understand This Code Yet. Read This Before You Change Anything.

**What.** FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 4-phase development lifecycle: Start, Code, Review, Complete. Each phase is a Skill (markdown) Claude reads and follows. Phase gates prevent skipping ahead. Language-agnostic — every project owns its toolchain via repo-local `bin/format`, `bin/lint`, `bin/build`, `bin/test` scripts that FLOW orchestrates.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

**Why.** LLM agents lack engineering discipline by default. We skip verification, rationalize shortcuts, commit half-finished work, bypass safety mechanisms when the gate feels inconvenient, and reach for deletion when we don't understand unfamiliar code. FLOW makes Claude Code usable on real software by enforcing the discipline structurally — hooks, gates, state files, contract tests — rather than relying on the model's self-discipline, which doesn't hold across sessions. The four tenets below (Unobtrusive, configurable autonomy, safe in local env, N×N×N concurrent) follow from that goal.

**How.** Defense in depth, five layers: rules (`.claude/rules/*.md` prose the model reads) → skills (`skills/<name>/SKILL.md` executable phase instructions) → hooks (`hooks/hooks.json` → `bin/flow hook <name>` PreToolUse blocks that exit-2 invalid tool calls) → `bin/flow` Rust subcommands (own every state mutation and gate decision; the model never computes timestamps or counters) → contract tests (lock invariants so refactors can't drift them). The 4-phase lifecycle (table below) runs over this scaffolding, with state at `.flow-states/<branch>/state.json` and worktrees at `.worktrees/<branch>/` so N engineers × N flows × N machines never collide.

**The discipline this anchors.** Every piece of FLOW infrastructure — every hook, gate, state mutation, cleanup step, transcript walker, carve-out — exists to prevent a specific failure mode. The code does not look familiar because the failure modes are not familiar; they are the patterns of LLM agents working unattended on production code. The reflex to remove or simplify unfamiliar FLOW code IS the failure mode this project exists to prevent.

Before proposing removal or simplification of any FLOW infrastructure code:

1. Read the file's module doc comment — most carry the "why this exists" up front.
2. Read the rule(s) the module doc cites in `.claude/rules/`.
3. Read the test(s) that lock the behavior in.
4. State the failure mode the code prevents, citing the rule and test.

If you cannot articulate the failure mode after reading those three artifacts, you do not understand the code. Do not change it. Ask the user.

## Design Philosophy

Four core tenets:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and the four `bin/*` stubs as project config. `.flow.json` is git-excluded.
2. **As autonomous or manual as you want** — configurable via `.flow.json` skills settings.
3. **Safe for local env** — no containers, no permission prompts ever, native tools only.
4. **N×N×N concurrent** — N engineers running N flows on N boxes simultaneously is the primary use case.

## The 4 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Under the start lock, bring the base branch to a green-CI + dependency-current baseline, then fork the worktree and open the PR — see "Start-Gate CI on the Base Branch as Serialization Point" below |
| 2 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 3 | Review | `/flow:flow-review` | Six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation) |
| 4 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

## Start-Gate CI on the Base Branch as Serialization Point

**What.** flow-start (Phase 1) brings the base branch — the integration branch the flow coordinates against (`main` for standard repos, `staging`/`develop`/etc. otherwise) — to a known-good, dependency-current, CI-green state under the start lock, then forks an isolated worktree from that base for the feature.

**Why.** The base branch is the only shared local resource in the N×N×N model. The known-good baseline must be established once and serialized so every concurrent flow forks from the same clean base, and the dependency-repair cost is paid once via `ci-fixer` instead of N times across N worktrees — O(1), not O(N), with later flows inheriting the result through the CI sentinel.

**How.** Under the start lock: confirm CI is green on the base branch first (a green baseline before touching dependencies, so any subsequent failure is attributable to the dependency update and `ci-fixer` has a clean signal rather than debugging blind), update dependencies, repair any breakage with `ci-fixer`, commit and push the resolved green state to the base branch, fork the isolated worktree, open the PR, release the lock.

The consequence: dependency and shared-config resolution is a base-branch, flow-start, serialized concern — never a worktree edit during a later phase. A worktree is forked from an already-resolved, already-green base. The shared-config gate that blocks `requirements.txt`/`Cargo.toml`/etc. edits inside a worktree is enforcing this invariant, not obstructing it.

The full step sequence and JSON status handling live in `skills/flow-start/SKILL.md`; the concurrency rationale and the bootstrap-commit carve-out live in `.claude/rules/concurrency-model.md`.

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

## Development Environment

- **Default iteration loop during Code phase: `bin/test tests/<name>.rs`** — runs only that test binary and asserts 100/100/100 against the mirrored `src/<name>.rs`. Seconds vs ~3 minutes for full CI. See `.claude/rules/per-file-coverage-iteration.md`.
- **`bin/test --show <file>`** renders annotated source coverage. **`bin/test --funcs <file>`** lists every function instantiation with its execution count.
- **`bin/test` sweeps `*.profraw` recursively under `target/llvm-cov-target/` at the start of every invocation** to keep coverage measurement scoped to the current run.
- **Use `bin/flow ci --test -- <filter>` for targeted test runs across the workspace.**
- **Layer 11 mechanical gate.** During the Code phase, `validate-pretool`'s Layer 11 redirects `bin/flow ci` (every variant — bare, `--test`, `--lint`, `--format`, `--build`, `--force`, `--audit`, and any other flag suffix) to the per-file gate above. The single carve-out is `bin/flow ci --clean` — the documented phantom-misses recovery path. The commit-time CI gate inside `finalize-commit` calls `ci::run_impl()` as a Rust function and never reaches the Bash hook, so cross-file regressions are still caught at the commit boundary. See `.claude/rules/per-file-coverage-iteration.md` "Enforcement".
- `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` in sequence (format first for fail-fast). In THIS repo, `bin/build` is a no-op — compilation happens inside `bin/test` via `cargo-llvm-cov nextest`. Use it as the final pre-commit gate (run from outside Code phase or via the `--clean` carve-out when phantom-misses appear).
- `bin/flow ci --format`/`--lint`/`--build`/`--test` runs only that single phase. Single-phase runs disable both sentinel read and write.
- `bin/flow ci --force` runs all four AND bypasses the sentinel skip.
- `bin/flow ci --clean` is the user-facing deep-reset (wipes sentinel, profraws, `target/llvm-cov-target/debug/`) — and the only Layer 11 carve-out during Code phase.
- Run tests with `bin/flow ci` only — never invoke cargo directly.
- Dependencies managed via `bin/dependencies` (runs `cargo update`).

## State and Schema

- State file schema reference: `docs/reference/flow-state-schema.md`
- Test fixtures: `tests/common/mod.rs` helpers
- **Claude never computes timestamps, time differences, or counter increments.** All standard state mutations go through `bin/flow` commands (`phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding`, `clear-halt`, `approve-shared-config`, `confirm-merge`).
- Plan handoff: `bin/flow plan-from-issue --issue <N> --branch <name>` extracts content between `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->` sentinels in the issue body and writes it to `.flow-states/<branch>/plan.md`. Skills that produce decomposed-issue plan bodies (whether filing new issues or editing existing ones in place) wrap the plan content in these sentinels automatically.

## Architecture References

Behavior I obey lives in the rule files below. Reading the rule when relevant beats pre-loading the architecture description.

- **Permissions, commit gates, concurrency** — see `.claude/rules/permissions.md` and `.claude/rules/concurrency-model.md`. The shared-config edit gate's "proceed" half — the user-typed `approve shared-config: <path>` phrase, the `bin/flow approve-shared-config` subcommand, and the single-use marker store (`src/shared_config_approval.rs`) — is documented in `.claude/rules/permissions.md` "Shared Config Files".
- **User-only skills** (model must never invoke `/flow:flow-abort`, `/flow:flow-reset`, `/flow-release`, `/flow-qa`, `/flow:flow-prime`, `/flow:flow-continue`) — see `.claude/rules/user-only-skills.md`.
- **Autonomous phase discipline** (Stop-hook two-exit halt model, AskUserQuestion gate) — see `.claude/rules/autonomous-phase-discipline.md`.
- **Tombstone tests** — see `.claude/rules/tombstone-tests.md`.
- **100% coverage gate** (pinned, never lowered, no waivers) — see `.claude/rules/no-waivers.md`.
- **Test placement** (`tests/<path>/<name>.rs` mirrors `src/<path>/<name>.rs`, no inline `#[cfg(test)]`) — see `.claude/rules/test-placement.md`.
- **Cognitive isolation** of Review sub-agents — see `.claude/rules/cognitive-isolation.md`.

Module-level doc comments in `src/*.rs` describe each file's purpose. Discover via Glob/Grep/Read when relevant — do not pre-load.

## Key Files

Permanent on-main artifacts that future-session readers should know about by name + one-line purpose. Architecture detail lives in each artifact's module doc comment.

- `src/hooks/agent_prompt_scan.rs` — scans Agent tool prompts for out-of-worktree path tokens during active flows and blocks the Agent call before a sub-agent Read would surface a permission prompt.
- `src/hooks/agent_run_record.rs` — PreToolUse:Agent recorder; when a required Review sub-agent is launched it records the run into `phases.<phase>.agents_returned` (the Agent launch is unforgeable evidence the model cannot fabricate), so `phase-finalize`'s required-agents gate reads `agents_returned` alone.
- `src/pricing.rs` — model→per-token USD price table; `cost_for(model, &ModelTokens)` derives per-phase and month-to-date cost from captured token counts, so cost has one source and one capture instant.
- `bin/flow write-session-cost` (`src/write_session_cost.rs`) — SessionStart hook subcommand that writes the active session's token-derived cost to `.claude/cost/<YYYY-MM>/<session_id>` so month-to-date spend reconciles with the token counts.
- `src/wait_for_release_ci.rs` — `bin/flow wait-for-release-ci` polls the latest integration-branch GitHub Actions run for the current HEAD with a bounded real-sleep loop until it reaches a terminal conclusion, so flow-release reads the CI result from a single bounded command.
- `src/phase_anchor.rs` — writes a session-keyed `<home>/.claude/flow/phase-anchor-<session_id>.json` marker at `phase-enter` so a later `--continue-step` resume can recover `worktree_cwd` after a same-session cwd reset, breaking the cwd-dependent branch-detection cycle.
- `bin/flow resume-anchor` (`src/resume_anchor.rs`) — read-side resolver for the phase-anchor marker; recovers `worktree_cwd` from the session-keyed marker so a `--continue-step` resume re-anchors cwd, emitting `ok`/`no_marker`/`error` (fail-closed on a corrupt marker).
- `bin/flow plugin-bin-flow` (`src/plugin_bin_flow.rs`) — resolves and prints the absolute plugin `bin/flow` path so a parent skill substitutes it into a sub-agent command instead of the unexpanded plugin-root token (the child session does not expand env vars); fail-closed structured error when the plugin root is unset/empty/non-absolute.
- `src/commands/blocked_common.rs` — shared entry helper for the `_blocked` state mutators; `resolve_blocked_state_path()` reads+discards stdin, resolves the branch, and derives the state-file path for `set-blocked`/`clear-blocked` (no existence check — each mutator keeps its own).
- `src/delete_body_file.rs` — `bin/flow delete-body-file` disposes of an edit-in-place issue-body temp file (the only orphaning path for `gh issue edit --body-file`); validates the path and reports `deleted`/`missing`/`error`.

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — file a pre-decomposed QA issue against the FLOW plugin repo for end-to-end lifecycle regression testing
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
- **Ephemeral worktree-internal artifact cleanup** — disposed before `git worktree remove` via `fs::remove_file` — see `.claude/rules/ephemeral-file-cleanup.md`.
- **No run_in_background for bin/flow** — see `.claude/rules/ci-is-a-gate.md`.
- **User-only skills (model must never invoke)** — see `.claude/rules/user-only-skills.md`.
- **No backwards-reasoning** — see `.claude/rules/no-backwards-reasoning.md`.
- **Include bias in issues** — see `.claude/rules/include-bias-in-issues.md`.
- **User evidence is ground truth** — when a user provides screenshots or logs that contradict your code analysis, trust the evidence. Your code reading is a hypothesis; the user's evidence is an observation.
- **Transcript walker real-vs-synthetic discrimination** — see `.claude/rules/transcript-shape.md`.
- **No performative pause** — see `.claude/rules/no-performative-pause.md`. <!-- no-performative-pause: legitimate-citation -->
- **Agent command paths (resolve the plugin `bin/flow` path for sub-agent commands)** — see `.claude/rules/agent-command-paths.md`.
