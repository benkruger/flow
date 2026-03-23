# CLAUDE.md

FLOW is a Claude Code plugin (`flow:` namespace) that enforces an opinionated 6-phase development lifecycle: Start, Plan, Code, Code Review, Learn, Complete. Each phase is a skill that Claude reads and follows. Phase gates prevent skipping ahead — you must complete each phase before entering the next. Supports Rails, Python, and iOS.

This repo is the plugin source code. When installed in a target project, skills and hooks run in the target project's working directory, not here. State files, worktrees, and logs all live in the target project. If you are developing FLOW itself, you are modifying the plugin — not using it.

## Design Philosophy

Four core tenets guide every design decision:

1. **Unobtrusive** — zero repo footprint, zero dependencies. Nothing is committed — `.claude/settings.json` and `.flow.json` are git-excluded. Everything else lives in `.git/` or is gitignored.
2. **As autonomous or manual as you want** — configurable autonomy via `.flow.json` skills settings.
3. **Safe for local env** — no containers needed, no permission prompts ever. Native tools only, no external dependencies.
4. **N×N×N concurrent** — N engineers running N flows on N boxes at the same time is the primary use case, not an edge case. Every feature, fix, and design decision must work when multiple flows are active simultaneously — on the same machine (multiple worktrees) and across machines (shared GitHub state). Local state (`.flow-states/`, worktrees) is per-machine. Shared state (PRs, issues, labels) is coordinated through GitHub. Nothing assumes a single active flow.

In the target project:

- Nothing is committed — `.claude/settings.json` and `.flow.json` are git-excluded
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
| 4 | Code Review | `/flow:flow-code-review` | Three built-in lenses (clarity, correctness, safety) plus configurable plugin (CLAUDE.md compliance) |
| 5 | Learn | `/flow:flow-learn` | Review mistakes, capture learnings, route to permanent homes |
| 6 | Complete | `/flow:flow-complete` | Merge PR, remove worktree, delete state file |

Phase gates are enforced by `lib/check-phase.py` — there is no instruction path to skip a phase. Back-transitions (e.g., Code Review can return to Code or Plan) are defined in `flow-phases.json`.

## When You Must Update Docs and Tests

"Marketing docs" refers to `docs/index.html` — the GitHub Pages landing page.

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
- `hooks/hooks.json` — hook registration (SessionStart, PreToolUse, PostToolUse, PostCompact, Stop)
- `hooks/session-start.sh` — detects in-progress features, injects awareness context
- `lib/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `.github/workflows/ci.yml` — GitHub Actions CI (runs `bin/ci` on push/PR to main)
- `.github/workflows/autoupdate.yml` — auto-updates PR branches when main advances
- `docs/` — GitHub Pages site (main /docs, static HTML)
- `lib/extract-release-notes.py` — extracts version sections from RELEASE-NOTES.md for GitHub Releases
- `lib/start-lock.py` — serializes concurrent flow-start operations using a file lock at `.flow-states/start.lock` (PID-based stale detection + 30-min timeout)
- `lib/init-state.py` — early state file creation with null PR fields for TUI visibility during Start; called before locked main operations
- `lib/start-setup.py` — consolidated Start phase setup (worktree, PR, state file backfill, repo detection; optional git pull via `--skip-pull`)
- `lib/flow_utils.py` — shared utilities: `now()` (Pacific Time timestamps), `PACIFIC` timezone, `format_time()`, `elapsed_since()`, `read_version()`, `read_version_from()`, `current_branch()`, `project_root()`, `read_flow_json()`, `extract_issue_numbers()`, `short_issue_ref()`, `read_prompt_file()`, `detect_repo()`, `mutate_state()`, `derive_feature()`, `derive_worktree()`, `freeze_phases()`, `build_initial_phases()`, `AUTO_SKILLS`, `PHASE_NAMES`, `COMMANDS`
- `lib/phase-transition.py` — phase entry/completion (timing, counters, status, formatted_time, phase_transitions recording, diff_stats capture)
- `lib/set-timestamp.py` — mid-phase timestamp fields via dot-path notation, code_task increment validation (prevents task batching)
- `frameworks/<name>/` — per-framework data: `detect.json`, `permissions.json`, `dependencies`, `priming.md`
- `lib/detect-framework.py` — data-driven framework auto-detection from `frameworks/*/detect.json`
- `lib/prime-project.py` — inserts framework conventions into target CLAUDE.md between markers
- `lib/create-dependencies.py` — copies framework dependency template to `bin/dependencies`
- `agents/ci-fixer.md` — custom plugin sub-agent for CI failure diagnosis and fix
- `lib/finalize-commit.py` — consolidates commit + message-file cleanup + pull + push into one subprocess chain
- `lib/generate-id.py` — generates an 8-character hex session ID via `uuid.uuid4().hex[:8]`; used by `flow-create-issue` and `flow-decompose-project` skills
- `lib/log.py` — appends timestamped entries to `.flow-states/<branch>.log` via Python file append with `fcntl.LOCK_EX` locking
- `lib/orchestrate-state.py` — manages `.flow-states/orchestrate.json` (create, start-issue, record-outcome, complete, read, next); uses `mutate_state` for atomic writes
- `lib/orchestrate-report.py` — generates morning report from orchestration state; writes `.flow-states/orchestrate-summary.md`
- `lib/analyze-issues.py` — mechanical analysis of open GitHub issues for the flow-issues skill: calls `gh issue list`, parses JSON, extracts file paths, detects `#N` dependencies, detects labels (Flow In-Progress, Decomposed), checks stale files, outputs condensed per-issue briefs as JSON
- `lib/close-issues.py` — closes GitHub issues referenced in the start prompt (`#N` patterns) via `gh issue close`
- `lib/label-issues.py` — adds or removes the "Flow In-Progress" label on GitHub issues referenced by `#N` in the start prompt; used by Start (add), Complete (remove), and Abort (remove) for multi-engineer WIP detection
- `lib/issue.py` — creates GitHub issues via `gh` subprocess (wraps `gh issue create`; resolves repo via `--state-file` cached value, then `--repo` flag, then git remote detection); returns `url`, `number`, and REST API `id` (database ID) for sub-issue linking
- `lib/create-milestone.py` — creates GitHub milestones via `gh api` (wraps `POST /repos/O/R/milestones`)
- `lib/create-sub-issue.py` — sets sub-issue parent/child relationships via `gh api` (resolves database IDs internally)
- `lib/link-blocked-by.py` — sets blocked-by dependency relationships via `gh api` (resolves database IDs internally)
- `lib/auto-close-parent.py` — checks if parent epic and milestone should be auto-closed when all children are done; best-effort throughout
- `lib/add-issue.py` — records filed issues in the state file's `issues_filed` array (follows `append-note.py` pattern)
- `lib/notify-slack.py` — posts messages to Slack via curl to `chat.postMessage`; reads config from `.flow.json`; supports threading via `thread_ts`; fails open on any error
- `lib/add-notification.py` — records sent Slack notifications in the state file's `slack_notifications` array (follows `add-issue.py` pattern)
- `lib/format-complete-summary.py` — generates the business-friendly Done banner for Complete phase (feature name, prompt, per-phase timeline, artifact counts)
- `lib/format-issues-summary.py` — formats `issues_filed` as a markdown table and banner line for Complete phase
- `lib/format-pr-timings.py` — reads state file, formats phase durations as a markdown table for PR body
- `lib/render-pr-body.py` — idempotent PR body renderer: reads state file + artifact files, generates complete body in canonical section order (What, Artifacts, Plan, DAG Analysis, Phase Timings, State File, Session Log, Issues Filed)
- `lib/update-pr-body.py` — updates PR body: `--add-artifact` for list items, `--append-section` for collapsible/plain sections
- `lib/stop-continue.py` — Stop hook script that forces continuation when `_continue_pending` flag is set in the state file; reads `_continue_context` for specific next-step instructions in the block reason
- `lib/post-compact.py` — PostCompact hook that captures `compact_summary`, `compact_cwd`, and `compact_count` in the state file for SessionStart to inject
- `lib/tui_data.py` — pure data layer for TUI: loads state files, computes flow summaries, phase timelines, parses log entries
- `lib/tui.py` — curses-based interactive TUI for viewing and managing active flows (`flow tui`)
- `lib/validate-ci-bash.py` — global PreToolUse hook validator (blocks compound commands, shell redirection, and file-read commands in all Bash calls)
- `lib/validate-ask-user.py` — PreToolUse hook on AskUserQuestion (blocks prompts when `_auto_continue` is set in state file; writes `_blocked` timestamp when allowing through)
- `lib/clear-blocked.py` — PostToolUse hook on AskUserQuestion that clears `_blocked` from the state file after the user responds; fail-open
- `lib/scaffold-qa.py` — creates QA repos from per-framework templates (`qa/templates/`); CLI: `bin/flow scaffold-qa --framework <name> --repo <owner/repo>`
- `lib/qa-reset.py` — resets QA repos to seed state (git reset, close PRs, delete branches, recreate issues); CLI: `bin/flow qa-reset --repo <owner/repo> [--local-path <path>]`
- `lib/qa-verify.py` — verifies QA assertions per tier (state files, phase completion, lock cleanliness, orphan detection); CLI: `bin/flow qa-verify --tier <N> --framework <name> --repo <owner/repo>`
- `lib/qa-mode.py` — manages dev-mode plugin_root redirection in `.flow.json` (backup/redirect/restore); CLI: `bin/flow qa-mode --start --local-path <path>` and `bin/flow qa-mode --stop`
- `qa/templates/<framework>/` — per-framework QA repo templates (rails, python, ios) with Calculator class, tests, bin/ci, and .qa/issues.json
- `bin/flow` — dispatcher script routing subcommands to `lib/*.py`
- `docs/reference/flow-state-schema.md` — state file schema reference
- `docs/reference/skill-pattern.md` — template pattern for building new phase skills
- `docs/integrations/slack.md` — Slack App setup guide and notification configuration
- `.claude-plugin/marketplace.json` — marketplace registry (version must match plugin.json)

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

### Local vs Shared State

FLOW's primary use case is N engineers running N flows on N boxes simultaneously. Every feature must work under these conditions:

- Multiple worktrees active on the same machine
- Multiple engineers working the same repo from different machines
- Multiple flows touching overlapping issues or files

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed — each machine has its own |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API — never assume local knowledge of other engineers' state |

State files (`.flow-states/`) are local to each machine. In a multi-engineer team, each engineer's `.flow-states/` only contains their own features. GitHub (issues, PRs, labels) is the shared coordination layer. The "Flow In-Progress" label on issues is the mechanism for cross-engineer WIP detection: Start adds it, Complete and Abort remove it, and `flow-issues` reads it from the existing label fetch.

### Sub-Agents

FLOW uses one custom plugin sub-agent: `ci-fixer` (`agents/ci-fixer.md`) for CI failure diagnosis and fix in Start (Steps 5 and 7) and Complete (Steps 4 and 5). Prompt-level tool restrictions are unreliable — sub-agents ignore them. The `PreToolUse` hook (`lib/validate-ci-bash.py`) is registered globally in `hooks/hooks.json`, blocking compound commands, shell redirection, and file-read commands in all Bash calls — including those from built-in skills' sub-agents. The ci-fixer also retains its own hook declaration for defense in depth.

Plan invokes the `decompose` plugin (`decompose:decompose`) for DAG-based task decomposition — no plan mode. Code Review uses three foreground review agents for clarity (code reuse, quality, efficiency), then delegates to built-in `/review`, `/security-review`, and optionally the `code-review:code-review` plugin for multi-agent validation (controlled by the `code_review_plugin` config axis: `"always"`, `"auto"`, or `"never"`). Code and Learn have no sub-agents. Complete uses ci-fixer for CI failures.

### Orchestration

`/flow:flow-orchestrate` is a meta-skill that processes decomposed issues overnight. It fetches open issues labeled "Decomposed", filters out "Flow In-Progress" issues, and runs each sequentially via `flow-start --auto`. State is tracked in `.flow-states/orchestrate.json` (a machine-level singleton, not branch-scoped). The session-start hook detects orchestrator state for both in-progress resume and completed morning report delivery. Only one orchestration runs per machine at a time.

### Memory and Learning System

Since Claude Code 2.1.63, auto-memory is shared across git worktrees of the same repository. Memory written during feature work persists at the repo-root path and survives worktree cleanup — no rescue needed.

Learn is a unified tri-modal skill. It auto-detects Phase 5 (state file with Code Review complete), Maintainer (no state file, `flow-phases.json` exists), or Standalone (no state file, no `flow-phases.json`). All three modes route learnings to 2 destinations. Phase 5 adds GitHub issues and phase transitions. Maintainer commits via `/flow:flow-commit --auto`. Standalone never commits.

The 2 destinations:

- **Project CLAUDE.md** (`CLAUDE.md` in project) — process rules, architecture, and conventions. Edited on disk, committed via PR.
- **Project rules** (`.claude/rules/<topic>.md` in project) — coding anti-patterns and gotchas. Edited on disk, committed via PR.

Learn also files GitHub issues for process gaps ("Flow" label on the plugin repo) and documentation drift ("Documentation Drift" label). All filed issues are recorded in the state file via `bin/flow add-issue` and surfaced in the Complete phase.

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

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-transition` for entry/completion, `set-timestamp` for mid-phase fields. `code_task` can only be incremented by 1 per call — even when multiple tasks are committed atomically, increment the counter one step at a time before committing. The plan file lives at `.flow-states/<branch>-plan.md` and its path is stored in `state["files"]["plan"]`. The DAG file (from decompose plugin) lives at `.flow-states/<branch>-dag.md` and is stored in `state["files"]["dag"]`. Legacy state files may still use top-level `state["plan_file"]` and `state["dag_file"]` — consumers should check `files` first with fallback to the top-level keys.

### Permission Invariant

Every `` ```bash `` block in every skill and docs file must run without triggering a Claude Code permission prompt. Two layers enforce this:

- **Test time** — `test_permissions.py` extracts every bash block, substitutes placeholders with concrete values, and verifies each command matches an allow-list pattern and does not match a deny-list pattern. New bash commands require a matching permission entry. New placeholders require a `PLACEHOLDER_SUBS` entry. Unrecognized placeholders fail the test — they are never silently skipped.
- **Runtime** — `validate-ci-bash.py` runs as a global `PreToolUse` hook on every Bash call. It blocks compound commands, shell redirection, and file-read commands via fast-path checks, then enforces the `.claude/settings.json` allow list as a whitelist. Commands not matching any `Bash(...)` allow pattern are blocked with exit code 2 and a helpful error message. If `settings.json` is missing (non-FLOW project), the whitelist check is skipped.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `target_project` (git repo with non-bash `bin/ci` and no `bin/flow` — simulates a Rails/non-Python target project), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files). Integration tests for lib scripts that run in target projects must use `target_project`, not `git_repo`.

| Test File | What It Enforces |
|-----------|------------------|
| `test_structural.py` | Config invariants: phases 1-6 exist, versions match across 3 locations, commands unique, hooks reference existing files |
| `test_skill_contracts.py` | SKILL.md content: HARD-GATE presence, announce banners, state updates, ci-fixer agent, logging sections, note-capture options. Uses glob-based discovery — new skills are automatically covered |
| `test_add_issue.py` | Issue recording: append to empty/existing array, missing state file, CLI integration |
| `test_notify_slack.py` | Slack notification: config reading, message formatting, curl posting, threading, fail-open behavior, CLI integration |
| `test_add_notification.py` | Notification recording: append to empty/existing array, message truncation, missing state file, CLI integration |
| `test_format_complete_summary.py` | Complete phase summary: basic summary, issues with #N shorthand, resolved issues (closed_issues param), notes, prompt truncation, format_time usage, borders, version fallback, CLI with --closed-issues-file |
| `test_format_issues_summary.py` | Issues summary formatting: empty/missing/single/multiple issues, label grouping, table output, CLI |
| `test_analyze_issues.py` | Issue analysis: file path extraction, dependency detection, label detection, stale detection, categorization, dependency graph, body truncation, CLI integration with gh subprocess/failure/timeout |
| `test_close_issues.py` | Issue closing: extraction of `#N` patterns from prompt, deduplication, partial failure, repo-based URL generation, no-repo fallback, CLI integration |
| `test_label_issues.py` | Issue labeling: add/remove Flow In-Progress label, partial failure, deduplication, missing prompt, CLI integration |
| `test_check_phase.py` | Phase guard: blocks on incomplete prerequisites, allows on complete, handles worktrees, re-entry notes |
| `test_session_start.py` | Session hook: feature detection, timing reset, awareness injection, multi-feature handling |
| `test_docs_sync.py` | Docs completeness: every skill has a docs page, every phase has a docs page, index and README mention all commands |
| `test_permissions.py` | Permission simulation: allow/deny coverage, placeholder validation, source-of-truth sync between prime-setup.py and flow-prime/SKILL.md, regex unit tests. Unrecognized placeholders fail loudly |
| `test_bin_ci.py` | CI runner: venv detection, pass/fail behavior |
| `test_bin_test.py` | Test runner: venv detection, pass/fail, argument passthrough |
| `test_init_state.py` | Init state: early state file creation, null PR fields, phase initialization, framework/skills propagation, auto flag, prompt storage, frozen phases, logging, branch name derivation, error cases, CLI integration |
| `test_start_lock.py` | Start lock: acquire/release/check, stale PID detection, timeout, corrupted lock handling, CLI integration |
| `test_start_setup.py` | Start setup script: branch naming, settings merge, worktree, state file backfill, logging, error paths, repo detection, subprocess timeouts |
| `test_phase_transition.py` | Phase entry/completion: timing, counters, status, formatted_time, phase_transitions recording, diff_stats capture |
| `test_set_timestamp.py` | Mid-phase timestamps: dot-path navigation, NOW replacement, code_task increment validation |
| `test_extract_release.py` | Release notes extraction from RELEASE-NOTES.md |
| `test_detect_framework.py` | Framework auto-detection: file patterns, multiple matches, defaults, CLI |
| `test_prime_project.py` | CLAUDE.md priming: marker insertion, idempotent replacement, framework switching |
| `test_create_dependencies.py` | Dependency template: file creation, skip-if-exists, chmod, CLI |
| `test_prime_setup.py` | Prime setup: data-driven permissions, settings merge, version marker, git exclude, pre-commit hook |
| `test_validate_ask_user.py` | AskUserQuestion hook: blocks prompts when `_auto_continue` set, allows when absent/empty, `_blocked` write on allow, subprocess integration |
| `test_clear_blocked.py` | PostToolUse hook: clears `_blocked` from state, noop when absent, fail-open on errors, subprocess integration |
| `test_post_compact.py` | PostCompact hook: compact_summary/cwd/count written to state, fail-open on errors, subprocess integration |
| `test_finalize_commit.py` | Commit finalization: happy path, commit/pull/push failures, merge conflict detection, message file cleanup, CLI |
| `test_generate_id.py` | Session ID generation: length, hex format, uniqueness, main stdout, CLI integration |
| `test_flow_utils.py` | flow_utils functions: format_time, project_root, current_branch, find_state_files, resolve_branch, derive_feature, derive_worktree, detect_repo, mutate_state, extract_issue_numbers, short_issue_ref, tab color/title/sequence formatting |
| `test_log.py` | Log append: existing file, new file, directory creation, multiple appends, file locking verification, CLI integration |
| `test_tui_data.py` | TUI data layer: load_all_flows (0/1/N files, corrupt JSON, phases exclusion), flow_summary (all fields), phase_timeline (statuses, annotations), parse_log_entries (parsing, limits, malformed), read_version |
| `test_tui.py` | TUI curses app: drawing (list/log/detail views), keyboard input (navigation, open worktree/PR, abort with confirm, refresh, quit), run loop (timeout, resize), edge cases (no flows, small terminal) |
| `test_update_pr_body.py` | PR body management: artifact lines, section insertion, collapsible sections, add-artifact/append-section modes, idempotent replacement, content-file reading, CLI integration |
| `test_orchestrate_state.py` | Orchestrate state: create, start-issue, record-outcome, complete, read, next, queue filtering, error paths, CLI |
| `test_orchestrate_report.py` | Orchestrate report: all completed, mixed, all failed, empty queue, timing, PR URLs, failure reasons, summary file, CLI |
| `test_concurrency.py` | Real-process concurrency: mutate_state counter integrity (20 workers), log append line integrity (20 workers), start-lock serialization (3 workers), parallel state file creation (5 workers), cleanup isolation (concurrent cleanup + mutation) |
| `test_scaffold_qa.py` | QA repo scaffolding: template discovery per framework, file writing, subprocess mocking for gh/git, bin/ci executable bit, error paths, CLI main() |
| `test_qa_reset.py` | QA repo reset: git reset, PR closing, branch deletion, issue template loading/recreation, local cleanup, error paths, CLI main() |
| `test_qa_verify.py` | QA verification: tier 1 (lifecycle checks, phase completion, PR merge), tier 2 (concurrent flow isolation, branch uniqueness), tier 3 (stale lock, orphan state files), error paths, CLI main() |
| `test_qa_mode.py` | QA dev-mode: start/stop happy paths, missing .flow.json, missing plugin_root, double start, invalid path, key preservation, CLI integration |

## Maintainer Skills (private to this repo)

- `/flow-qa` — `.claude/skills/flow-qa/SKILL.md` — `--start`/`--stop` dev mode, `--run`/`--reset`/`--tier` 3-tier QA protocol against per-framework repos. **Always run `/flow-qa --start` before `/flow:flow-start` when developing FLOW.** The installed marketplace plugin enforces its own phase count and skill gates, which conflict with the source being developed and break the workflow mid-feature.
- `/flow-release` — `.claude/skills/flow-release/SKILL.md` — bump version, tag, push, create GitHub Release

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
- **Prefer dedicated tools over Bash for all non-execution tasks** — Read files with the Read tool, search with Glob and Grep, create with Write, modify with Edit. Bash should only be used for commands that genuinely require shell execution: `bin/ci`, `bin/test`, `bin/flow`, `make`, and `git`. In this project's strict permission environment (`defaultMode: "plan"`), every Bash command not in the allow list triggers a permission prompt. When you need to explore, understand, or modify files, use dedicated tools — they never prompt.
- **Always use `bin/flow issue` to file GitHub issues** — never use `gh issue create` directly. `bin/flow issue` auto-detects the repo from git remote when `--repo` is omitted; pass `--repo` only when filing against a different repo. Direct `gh` calls trigger permission prompts.
- **All FLOW-produced rules and instructions target the project repo** — `CLAUDE.md` and `.claude/rules/` are always repo-level paths, never user-level `~/.claude/` paths. Reading user-level files is fine; writing to them is never valid during any FLOW phase.

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
