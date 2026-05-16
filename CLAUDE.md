# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 5-phase development lifecycle: Start, Code, Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead. Language-agnostic — every project owns its toolchain via repo-local `bin/format`, `bin/lint`, `bin/build`, `bin/test` scripts that FLOW orchestrates.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets:

1. **Unobtrusive** — zero dependencies. Prime commits `.claude/settings.json` and the four `bin/*` stubs as project config. `.flow.json` is git-excluded.
2. **As autonomous or manual as you want** — configurable via `.flow.json` skills settings.
3. **Safe for local env** — no containers, no permission prompts ever, native tools only.
4. **N×N×N concurrent** — N engineers running N flows on N boxes simultaneously is the primary use case.

## The 5 Phases

| Phase | Name | Command | Purpose |
|-------|------|---------|---------|
| 1 | Start | `/flow:flow-start` | Create worktree, PR, state file, configure workspace |
| 2 | Code | `/flow:flow-code` | Execute plan tasks one at a time with TDD |
| 3 | Review | `/flow:flow-review` | Six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation) |
| 4 | Learn | `/flow:flow-learn` | Capture learnings, route to permanent homes |
| 5 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

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

- Run tests with `bin/flow ci` only — never invoke cargo directly
- `bin/flow ci` runs `./bin/format`, `./bin/lint`, `./bin/build`, `./bin/test` in sequence (format first for fail-fast). In THIS repo, `bin/build` is a no-op — compilation happens inside `bin/test` via `cargo-llvm-cov nextest`.
- `bin/flow ci --format`/`--lint`/`--build`/`--test` runs only that single phase. Single-phase runs disable both sentinel read and write.
- `bin/flow ci --force` runs all four AND bypasses the sentinel skip.
- `bin/flow ci --clean` is the user-facing deep-reset (wipes sentinel, profraws, `target/llvm-cov-target/debug/`).
- **For single-file coverage iteration, use `bin/test tests/<name>.rs`** — runs only that test binary and asserts 100/100/100 against the mirrored `src/<name>.rs`. Seconds vs ~3 minutes for full CI. See `.claude/rules/per-file-coverage-iteration.md`.
- **Use `bin/flow ci --test -- <filter>` for targeted test runs across the workspace.**
- **`bin/test` sweeps `*.profraw` recursively under `target/llvm-cov-target/` at the start of every invocation** to keep coverage measurement scoped to the current run.
- **`bin/test --show <file>`** renders annotated source coverage. **`bin/test --funcs <file>`** lists every function instantiation with its execution count.
- Dependencies managed via `bin/dependencies` (runs `cargo update`).

## State and Schema

- State file schema reference: `docs/reference/flow-state-schema.md`
- Test fixtures: `tests/common/mod.rs` helpers
- **Claude never computes timestamps, time differences, or counter increments.** All standard state mutations go through `bin/flow` commands (`phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, `add-finding`, `add-skipped-agent`, `record-agent-return`, `clear-halt`).
- Plan handoff: `bin/flow plan-from-issue --issue <N> --branch <name>` extracts content between `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->` sentinels in the issue body and writes it to `.flow-states/<branch>/plan.md`. Issue-filing skills wrap their output in these sentinels automatically.

## Architecture References

Behavior I obey lives in the rule files below. Reading the rule when relevant beats pre-loading the architecture description.

- **Permissions, commit gates, concurrency** — see `.claude/rules/permissions.md` and `.claude/rules/concurrency-model.md`.
- **User-only skills** (model must never invoke `/flow:flow-abort`, `/flow:flow-reset`, `/flow-release`, `/flow-qa`, `/flow:flow-prime`, `/flow:flow-continue`) — see `.claude/rules/user-only-skills.md`.
- **Autonomous phase discipline** (Stop-hook two-exit halt model, AskUserQuestion gate) — see `.claude/rules/autonomous-phase-discipline.md`.
- **Tombstone tests** — see `.claude/rules/tombstone-tests.md`.
- **100% coverage gate** (pinned, never lowered, no waivers) — see `.claude/rules/no-waivers.md`.
- **Test placement** (`tests/<path>/<name>.rs` mirrors `src/<path>/<name>.rs`, no inline `#[cfg(test)]`) — see `.claude/rules/test-placement.md`.
- **Cognitive isolation** of Review/Learn sub-agents — see `.claude/rules/cognitive-isolation.md`.

Module-level doc comments in `src/*.rs` describe each file's purpose. Discover via Glob/Grep/Read when relevant — do not pre-load.

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
