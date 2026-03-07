# Release Notes

## v0.19.0 — Namespace cleanup and landing page overhaul

### Breaking Changes

- **Skill names prefixed with `flow-`**: All skill directories, phase keys, state file keys, and commands now use `flow-` prefix (`/flow:flow-start`, `/flow:flow-plan`, etc.) to eliminate autocomplete collisions with Claude Code built-ins. Existing state files with bare phase keys need manual update.
- **Phase identifiers use name-based keys**: State file phase keys changed from numeric (`"1"`, `"2"`) to name-based (`"flow-start"`, `"flow-plan"`). Existing state files need manual update.

### New Features

- **Upgrade check in Start phase**: `/flow:flow-start` now notifies users when a newer FLOW version is available
- **Standalone commit skips CI**: Commit skill in Standalone mode skips `bin/flow ci` since it's not available outside the plugin repo

### Fixes

- **Plan file ordering**: `plan_file` is now stored in state before `ExitPlanMode`, not after — fixes null `plan_file` when user chooses "clear context and proceed"
- **Security commit axis**: Security phase was missing its `commit` axis configuration
- **Bash file commands denied**: `cat`, `head`, `tail`, `find`, `grep` via Bash now trigger deny prompts — enforces dedicated tool usage

### Improvements

- **Landing page overhaul**: Replaced dense technical documentation with clear selling-point messaging. Flip cards replaced with static "Why FLOW Is Different" cards. Pipeline descriptions rewritten in plain English.
- **Key feature coverage tests**: New tests enforce that README and landing page mention all key selling points (autonomy, learning system, plan mode, zero dependencies, etc.)
- **README selling points**: Added "Why FLOW" section with concise bullet-point differentiators

## v0.18.0 — Rename Phase 7 from Reflect to Learning

### Breaking Changes

- **Phase 7 renamed:** `flow:reflect` is now `flow:learning` across all skills, docs, state files, and phase definitions. Existing state files referencing "reflect" will need manual update.
- **All skills renamed with `flow-` prefix:** Skill directories, phase keys, state file keys, and commands now use `flow-` prefix (`/flow:flow-start`, `/flow:flow-plan`, etc.) to avoid namespace collisions with Claude Code's built-in autocomplete. Existing state files with bare phase keys (`"start"`, `"plan"`) will need manual update.

### New Features

- **Centralized constants:** COMMANDS and PHASE_NAMES moved to `flow_utils.py` as the single source of truth for all scripts

### Fixes

- **Block cd && compound commands:** Added "never cd before bin/flow" guardrail to all 10 phase skill Hard Rules, and "never cd && git" prohibition to Review, Security, and Start sub-agent Tool rules. Prevents Claude Code's "bare repository attacks" permission prompts
- **QA --restart fallback:** `/qa --restart` now falls back to `--start` when not in dev mode instead of refusing
- **Commit default-auto:** Made `--auto` default impossible to miss at the decision point in commit skill
- **Heredoc elimination:** All banner instructions now say "output in response" instead of using Bash print/heredoc

### Improvements

- New `test_no_cd_compound_in_bash_blocks` catches cd && patterns in bash blocks before they ship
- Document autonomy configuration as a headline feature in README
- Document CLAUDE.md audit requirement for codebase-wide renames in skill-authoring rules

## v0.17.0 — Two-axis autonomy and /flow:config

### New Features

- **Two-axis autonomy model** — `commit` and `continue` are now independently configurable per skill. The commit axis controls diff approval during phase work; the continue axis controls phase advancement. Presets: fully autonomous, fully manual, recommended, or customize per skill.
- **`/flow:config` display skill** — Shows the current autonomy configuration from `.flow.json` in a table with Commit and Continue columns per skill
- **`/qa --restart` flag** — Refreshes the plugin cache without leaving dev mode, saving two cache rebuilds when iterating on local changes

### Improvements

- `/flow:commit` now defaults to auto — removed from configurable skills since invoking it directly is already a user choice; `--manual` available as opt-in gate
- Plan skill uses plain text prompt instead of AskUserQuestion for plan approval
- Auto-continue after ExitPlanMode clears context, so new sessions pick up where they left off
- Desktop screenshot reading allowed without permission prompts
- Init skill can re-initialize at any time (no longer blocked by existing `.flow.json`)
- Init eliminates permission prompts during staged-changes check
- Removed dead permission entry for `~/.claude/` sensitive path

## v0.16.4 — Enforce tool restrictions in phase skills

### Fixes

- Add tool restriction Hard Rules to all 8 phase skills — Claude was ignoring passive rules in `.claude/rules/` and using `ls`, `cd && git`, and piped `git show | sed` during QA. Co-locating the restriction in each skill's Hard Rules puts it where Claude actively reads
- Allow `git show` in Review/Security sub-agent prompts — sub-agents need it to compare files against `origin/main`
- Ban piping git output through `sed`/`grep`/`awk` in sub-agent prompts — piped commands triggered permission prompts
- Three new contract tests enforce the above: `test_subagent_prompts_allow_git_show`, `test_subagent_prompts_ban_piping`, `test_phase_skills_have_tool_restriction_in_hard_rules`

### Improvements

- Configure FLOW workspace permissions and version marker via `/flow:init`

## v0.16.3 — Reflect learning system overhaul

### Improvements

- Reorganize learnings into rules files for better routing
- Update Reflect routing heuristics and remove obsolete worktree memory rescue
- Report all obsolete terms at once instead of failing on first match

### Fixes

- Fix release skill false "nothing to release" short-circuit
- Fix review findings: update missed docs page and add trailing newline

## v0.16.2 — QA bug fixes

### Fixes

- Fix stale COMMANDS dict in format-status.py — phase 4+ showed wrong
  commands since Simplify was added (7 entries instead of 8)
- Switch format-status output from JSON to plain text with exit codes —
  eliminates noisy `\n`-literal JSON in Bash tool collapsed output
- Replace `Bash(cd .worktrees/* && *)` permission with `Bash(git -C *)`
  — Claude Code's "bare repository attacks" heuristic fires on any
  `cd <path> && git` compound command regardless of the allow list
- Add "Provide exactly these options" constraint to Plan skill
  AskUserQuestion — prevents Claude adding unauthorized options
- Document Read tool permission prompts for `~/.claude/` paths in
  Reflect skill as a known limitation
- Add `.claude/rules/worktree-commands.md` — instructs Claude to use
  `git -C` and dedicated tools instead of bash workarounds

## v0.16.1 — Fix Start phase step ordering to prevent parallelization

### Fixes

- Reorder Start Steps 1-2: version gate now runs first (cheapest check),
  existing feature check second
- Wrap both early steps in `<HARD-GATE>` tags to prevent Claude from
  launching `bin/flow ci` before gates resolve

## v0.16.0 — Add Simplify phase

### New Features

- **Simplify phase (phase 4)** — New phase invokes `/simplify` on committed code,
  reviews diff, and auto-commits improvements. 8-phase architecture: Start, Plan,
  Code, Simplify, Review, Security, Reflect, Cleanup.

### Fixes

- Fix permissions, back-navigation, and stale references found during Review
- Gate release skill on empty commit list — prevents false "nothing to release"
  when commits exist
- Make commit mode detection a direct Read command instead of intent description —
  prevents tool substitution

### Improvements

- All docs, references, phase numbers, and CLAUDE.md updated for 8-phase structure
- Reflect learnings captured: back-navigation audit and deny-list checks

---

## v0.15.0 — Post-flow improvements

### New Features

- **Reflect: settings.local.json audit (Maintainer mode)** — After a FLOW cycle,
  permissions added via "Always allow" land in settings.local.json. Reflect now
  reads that file, asks which entries to promote to settings.json, and deletes it.
- **Cleanup: pull main after finishing** — Local main now has the merged feature
  code when cleanup completes, so the next /flow:start begins from up-to-date main.

### Improvements

- **Plan enters plan mode earlier** — EnterPlanMode now runs immediately after
  Update State, before asking the user what to build. The entire planning flow
  runs inside plan mode where no file edits are possible.
- **Standardized bin/flow ci across all skills** — All skill bash blocks and prose
  now use `bin/flow ci` (or `bin/flow ci --if-dirty`) instead of bare `bin/ci`.
  Consistent dispatcher usage across Start, Code, Review, Security, and Commit.
- **Unified commit skill** — Maintainer and flow:commit merged into a single
  tri-modal skill (FLOW, Maintainer, Standalone). Version gate moved before bin/ci
  for fast feedback on stale /flow:init.

## v0.14.0 — Collapse phases, unify Reflect, speed up tests

### New Features

- Collapse Research, Design, and Plan into a single Plan phase using Claude Code's native plan mode
- Unify Reflect into one tri-modal skill (Phase 6, Maintainer, Standalone)
- Add `/reset` maintainer skill to wipe all FLOW artifacts
- Add `--if-dirty` flag to `bin/ci` to skip redundant runs between phase transitions
- Auto-approve commits for Python projects

### Improvements

- Make Reflect phase fully autonomous (no approval prompts)
- Make `flow:cleanup` skip confirmation by default
- Session hook is now awareness-only — stops auto-invoking `flow:continue`
- Split QA skill into `--start`/`--stop` with dev mode tracking
- Eliminate computational instructions from skills, switch to Pacific Time
- Skip redundant CI runs between phase transitions

### Performance

- Speed up test suite from 18s to ~6s across two optimization PRs
- Eliminate subprocess overhead in multiple test files (in-process calls)
- Template-copy pattern for git repo fixtures
- Enable parallel test execution with pytest-xdist

### Fixes

- Fix state file lookup after `/clear` resets branch to main
- Fix off-by-one in status panel "Next" command
- Fix sub-agent permissions, framework narration, and QA issues across 3 rounds
- Fix `.flow-commit-msg` being tracked in commits
- Fix `.gitignore` to suppress `.venv` symlinks

---

## v0.13.1 — Permission prompt fixes and simulation tests

**Fixes**

- Fixed permission prompts during FLOW execution by adding `defaultMode: acceptEdits`, substituting placeholders instead of skipping them, and adding `git pull origin *` to the allow list
- Added init skill for one-time workspace setup (permissions merge, version marker, git excludes)

**Improvements**

- Added 7 permission simulation tests: unrecognized placeholder detection, deny-list collision checks, allow/deny overlap detection, regex converter unit tests, and init-setup.py / init/SKILL.md sync enforcement
- Documented the Permission Invariant in CLAUDE.md

---

## v0.13.0 — Multi-framework support

FLOW now supports Rails and Python projects. `/flow:init` asks which framework
the project uses and configures permissions accordingly. Each phase skill loads
framework-specific content from fragment files (`skills/<phase>/rails.md` and
`skills/<phase>/python.md`), keeping the shared workflow in SKILL.md.

- **New**: Framework question in `/flow:init` — writes `framework` to `.flow.json`
- **New**: Framework-aware permissions — Rails and Python each get their own Bash allow lists
- **New**: Framework propagation — `framework` flows from `.flow.json` into the state file at Start
- **New**: 7 skill fragment pairs (Start, Research, Design, Plan, Code, Review, Security) — each with Rails and Python variants covering architecture checks, sub-agent prompts, design categories, plan sections, and security checks
- **New**: Test infrastructure extended to scan fragment files for permission coverage, JSON validity, sub-agent tool restrictions, and fragment pairing
- **Improved**: Docs, README, and plugin metadata updated for multi-framework support

---

## v0.12.0 — Light mode for bug fixes

- **New**: `--light` flag on `/flow:start` — for bug fixes and small changes that don't need full Design ceremony. Skips Phase 3: Design. Research uses a "recent changes first" protocol (git history before deep exploration) and writes a simplified design object so Plan and Review work unchanged.
- **New**: State file gains `mode: "light"` (top-level) and `skipped: true` (on Design phase)
- **New**: `/flow:status` shows `[~]` marker for skipped phases
- **New**: Session hook appends "(light mode)" to resume context
- 9 new tests, 100% coverage maintained

---

## v0.11.0 — Multi-destination learning routing

- **New**: Reflect routes learnings to 5 destinations — each approved learning goes to the right permanent home: global CLAUDE.md, project CLAUDE.md, global rules, project rules, or project memory. Claude recommends a destination; user confirms or overrides with one click.
- **New**: Worktree auto-memory rescue — Reflect reads Claude's auto-memory from the worktree before Cleanup destroys it, surfacing useful patterns as "Worth preserving" proposals that get routed to permanent storage.
- **New**: 4th source — Reflect now synthesises from four sources instead of three (state file, notes, conversation, worktree memory).
- **Fixed**: Logging wrapper that broke permissions and shell persistence.

---

## v0.10.0 — Auto-mode flags for commit and release

- **New**: `/flow:commit --auto` skips the approval prompt — user-invoked only, skills cannot call it programmatically
- **New**: `/release` now auto-proceeds by default (no approval pause at Step 5); `--manual` flag restores the old approval prompt for version overrides or dry-runs

---

## v0.9.0 — Security phase with 10 actionable checks

- **New**: Phase 7: Security — mandatory Explore sub-agent runs 10 security checks against the feature diff: authorization gaps, unscoped record access, mass assignment, SQL injection, data exposure, CSRF bypass, open redirects, RuboCop disables, auth test coverage, and route exposure
- **New**: Per-finding commit workflow — each confirmed finding is fixed, tested with bin/ci, and committed individually
- **New**: State tracking — findings and clean checks recorded in `state["security"]` for resumability across sessions
- **Changed**: No severity tiers — every confirmed finding gets fixed
- **Changed**: No back-navigation from Security — security issues are code-level fixes, not design problems

---

## v0.8.5 — Performance and cleanup

- **Renamed**: `/flow:resume` → `/flow:continue` to avoid conflict with Claude Code's built-in `/resume` command
- **Optimized**: Reduced redundant tool calls across 6 skills — Research, Design, Plan, Code, Reflect, and Continue — cutting unnecessary file reads at phase entry
- **Improved**: `/flow:status` upgraded to zero-arg CLI with richer panel output
- **Improved**: `/flow:note` made zero-arg, `PHASE_NAMES` deduplicated into shared constant
- **Fixed**: Stale state file paths in Research docs
- **Fixed**: `/flow:cleanup` — removed redundant read, dead logging code, stale docs references
- **Fixed**: `/flow:abort` — removed PR comment step, eliminated duplicate state reads
- **Added**: `psql` to user workspace permissions
- **Added**: CLAUDE.md rule — search before claiming something doesn't exist

---

## v0.8.4 — Performance and bug fixes

### Improvements

- Speed up `/release` skill: merge version and release notes into one
  prompt, replace `/commit` delegation with direct commit-and-push,
  reduce CI polling from 30s to 15s
- Enforce automatic model selection via skill frontmatter
- Add goal-over-mechanism rule to commit subject line guidelines
- Consolidate 7 Bash permission entries into 1 via `bin/flow` dispatcher

### Fixes

- Fix missing trailing newlines in `settings.json` and `.flow.json`
  written by `flow:init`
- Add `git push` to `flow:init` so changes reach the remote
- Handle `flow:init` re-runs gracefully (skip commit when nothing
  changed)

## v0.8.3 — Extract testable Python from five skills

### New

- **`hooks/flow_utils.py`** — Shared utility module with `format_time()`,
  `project_root()`, and `current_branch()`. Used by `check-phase.py` and
  `format-status.py`.
- **`hooks/format-status.py`** — Deterministic ASCII status panel formatter,
  replacing inline template in `/flow:status`.
- **`hooks/append-note.py`** — Structured note appender for state files,
  replacing inline JSON manipulation in `/flow:note`.
- **`hooks/cleanup.py`** — Best-effort cleanup orchestrator shared by
  `/flow:cleanup` and `/flow:abort`, replacing duplicated inline sequences.
- **`hooks/init-setup.py`** — Settings merge, version marker, and git exclude
  setup for `/flow:init`.

### Improvements

- **`check-phase.py` refactored** — Imports `project_root()` and
  `current_branch()` from `flow_utils.py` instead of defining its own copies.
- **68 new tests** — Full test coverage for all five new scripts
  (219 total, 100% coverage).

### Fixes

- **Script paths use `${CLAUDE_PLUGIN_ROOT}`** — All five new scripts use
  the plugin root variable so they resolve correctly when users run skills
  from their project directory.

---

## v0.8.2 — Automate version bumps with make bump

### New

- **`make bump` target** — `make bump NEW=0.9.0` updates the version string
  in `plugin.json`, `marketplace.json`, and all 14 skill file banners in one
  command. Replaces the 14 manual `replace_all` edits the release skill
  previously required.
- **`hooks/bump-version.py`** — Standalone script with semver validation,
  same-version protection, and a summary of changed files. Full test coverage
  in `tests/test_bump_version.py`.

### Improvements

- **Release skill Step 6 simplified** — Now runs `make bump NEW=<version>`
  instead of listing 4 file groups to edit manually.
- **`Bash(make *)` permission added** — `make` commands are auto-allowed in
  `.claude/settings.json`.

---

## v0.8.1 — Fix /flow:init UX issues

### Fixes

- **Version marker moved out of .claude/** — `/flow:init` wrote `.claude/flow.json`,
  but Claude Code protects the `.claude/` directory and triggered a permission
  prompt. Moved to `.flow.json` in the project root.
- **Setup error output cleaned up** — `start-setup.py` printed error messages to
  both stdout (JSON) and stderr (raw text), then exited 1. The Bash tool showed
  a red "Error: Exit code 1" banner with duplicated text. Now exits 0 for all
  handled errors — the JSON `"status": "error"` is the signal, not the exit code.

---

## v0.8.0 — One-time project setup with /flow:init

### New Features

- **`/flow:init` skill** — New utility skill that runs once after installing
  or upgrading FLOW. Configures workspace permissions in `.claude/settings.json`,
  sets up git excludes for `.flow-states/` and `.worktrees/`, writes a version
  marker to `.flow.json`, and commits. Solves the chicken-and-egg problem
  where permissions written mid-session were never picked up because Claude Code
  snapshots settings at startup.
- **Version gate in `/flow:start`** — `start-setup.py` now checks
  `.flow.json` before any setup work. If FLOW hasn't been initialized or
  the version doesn't match, the user gets a clear error directing them to run
  `/flow:init`. This ensures permissions stay current across upgrades.

### Improvements

- **Settings logic removed from start-setup.py** — `_configure_settings()`,
  `_configure_exclude()`, and worktree settings copy all removed. Permissions
  are committed once via `/flow:init` and inherited by worktrees automatically.
- **Start skill simplified** — Removed the Read+Write settings reload hack and
  the "Reference: Workspace Permissions" section. Start now focuses on git,
  worktree, PR, and state file creation.
- **README and docs updated** — Installation instructions now include
  `/flow:init` as a required step. "Zero Footprint" updated to "Minimal
  Footprint" to acknowledge the committed `.claude/settings.json` and
  `.flow.json`.

---

## v0.7.3 — Fix workspace permissions in worktrees

### Fixes

- **Worktree permissions** — `start-setup.py` writes FLOW workspace permissions
  to `.claude/settings.json` in the project root, but `git worktree add`
  populates the worktree from HEAD (the committed version without FLOW entries).
  Every FLOW command after `cd .worktrees/<branch>` triggered a permission
  prompt. The script now copies the merged settings file into the worktree's
  `.claude/` directory after creation.
- **Settings reload** — Added a Read+Write reload step in the Start skill after
  `cd` into the worktree. This triggers Claude Code to detect and apply the
  copied permission entries before any commands run.
- **Release skill bypassed /commit** — Step 8 had its own `git commit`
  instructions instead of invoking `/commit`. This skipped `bin/ci`, diff
  review, and approval. Step 8 now delegates to `/commit`.

---

## v0.7.2 — Banner consistency fixes

### Fixes

- **Time formatting** — Completion banners now show formatted time (`3m`, `1h 5m`)
  instead of raw seconds (`235s`). All 8 phase COMPLETE banners use
  `<formatted_time>` with the same format spec as the status panel.
- **Suppress timing computation** — Added "Do not print the calculation"
  to phases 1-7 state update sections. Prevents Claude from showing
  work like "Phase 1 started at 07:35:12Z, now 07:39:07Z = 235 seconds."
  before the completion banner.
- **Version in all banners** — All STARTING and COMPLETE banners across
  all 12 skill files now include the version (`FLOW v0.7.2`). Previously
  only Start and Status showed it.

### Improvements

- **Release skill covers all skills** — Step 6 now replaces version
  across every `skills/*/SKILL.md` and `.claude/skills/release/SKILL.md`
  instead of just Start and Status.
- **6 new contract tests** — Enforce version in announce/complete banners,
  formatted_time usage, time format instructions, and output suppression.

---

## v0.7.1 — Fix Start phase permission prompt regression

### Fixes

- **Start logging pattern** — The Start phase consolidation (v0.7.0)
  reintroduced `$(date -u ...)` command substitution in the logging bash
  block. Claude Code flags `$()` with a security prompt that settings.json
  cannot suppress, blocking Start at Step 3. Restored the Read+Write
  pattern every other skill uses.

### Improvements

- **Command substitution regression test** — New test in test_permissions.py
  bans `$(` in any bash block across all SKILL.md and docs files. Would have
  caught this regression at CI time.
- **Release skill marketplace update test** — Enforces that the release skill
  includes the `claude plugin marketplace update` step.
- **CLAUDE.md lessons** — Added lesson on reporting unexpected conflicting
  tests when bin/ci reveals scope expansion beyond the plan.

---

## v0.7.0 — Start phase consolidation

### New Features

- **Consolidated setup script** — Start phase Steps 2-7 (git pull, settings
  merge, worktree creation, git exclude, empty commit+push+PR, state file
  creation) consolidated into a single Python script (`hooks/start-setup.py`).
  Reduces ~15 API round-trips to ~5, eliminating ~1m46s of LLM overhead.
- **`bin/test` wrapper** — New pytest wrapper for targeted test runs during
  development. Matches `bin/ci` pattern with venv detection.

### Fixes

- **Start phase PR creation** — Fixed PR creation failing when run from the
  wrong directory with insufficient commits.
- **CI fixture default branch** — Fixed `git_repo_with_remote` fixture failing
  in GitHub Actions by explicitly setting `-b main` on bare repo init.

### Improvements

- **Start SKILL.md rewritten** — Reduced from 12 steps to 7. Logging changed
  from Read+Write tool pattern to Bash append (`>>`).
- **CLAUDE.md lessons** — Added lessons on bin/test usage, test-first for all
  changes, plan-before-editing, scoping fixes, and never removing safety checks.
- **Release skill** — Restored automated marketplace update step that was
  incorrectly removed in a previous session.

---

## v0.6.5 — Permission hardening, phase timing, and markdown linting

### New Features

- **Phase timing in banners** — COMPLETE banners and the status panel now
  show elapsed time for each phase.
- **Markdown linting** — `bin/ci` now runs pymarkdownlnt before pytest.
  Re-enabled MD041 (first-line heading) now that frontmatter is handled.

### Security

- **Destructive git commands denied** — `git reset --hard`, `git stash`,
  `git checkout`, and `git clean` are now denied in both workspace and
  maintainer permission sets.
- **Permission deny list test** — New test in `test_permissions.py`
  validates deny entries exist for destructive operations.
- **Read-only shell utilities allowed** — `wc`, `sort`, `uniq`, and similar
  read-only commands added to maintainer permissions.
- **bypassPermissions banned** — Sub-agents must never use
  `bypassPermissions` mode. Lesson captured in CLAUDE.md.

### Improvements

- **Workspace permissions reordered** — Moved before worktree creation in
  Start so permissions apply from the first command.
- **Shared process docs inlined** — Eliminated shared doc references in
  favor of inline instructions in each skill.
- **Permission patterns fixed** — Corrected patterns that didn't match
  actual commands.
- **Marketplace update step** — Release skill now runs
  `claude plugin marketplace update` after creating the GitHub Release.
- **CLAUDE.md lessons** — Added TDD-first, plan-before-editing, and
  bypassPermissions lessons from reflect sessions.

---

## v0.6.4 — Security hardening and bug fixes

### Fixes

- **State file path** — Moved state files from `.claude/flow-states/` to
  `.flow-states/` to avoid Claude Code's built-in `.claude/` directory
  protections that triggered permission prompts.
- **Start phase worktree cd** — Fixed repeated `cd .worktrees/` breaking
  push and PR creation by using a single bare `cd` and relying on the
  Bash tool's persistent working directory.
- **State file cleanup** — Fixed `rm` permission for state files inside
  `.claude/flow-states/` (now `.flow-states/`).

### Security

- **Permission wildcards tightened** — Replaced `python3 *` with two
  specific script paths, removed unused `chmod *`, `env *`, `open *`
  wildcards, tightened `git rm *` to `.flow-commit-*` only, tightened
  `git pull *` to `git pull origin *` (blocks `--rebase`).
- **Force push denied** — Added explicit deny rules for `git push --force`
  and `git push -f`.
- **JSON escaping** — Replaced hand-rolled bash `escape_for_json()` in
  the session hook with Python's `json.dumps()` for proper escaping of
  all character classes.
- **Version validation** — Added semver format validation to
  `extract-release-notes.py` before using the version in file paths.
- **Abort permission** — Added `git branch -D *` to target project
  permissions so `/flow:abort` doesn't prompt.

### Improvements

- **Plan mode default** — Set `defaultMode: plan` in settings.json for
  maintainer sessions.

---

## v0.6.3 — CLAUDE.md architecture documentation

### Improvements

- **Architecture section** — New section documenting plugin vs target project,
  skills-are-markdown, shared process docs pattern, state file schema pointers,
  sub-agent architecture, logging pattern, and version locations.
- **Test Architecture section** — New section mapping each test file to what it
  enforces, plus shared fixture inventory.
- **Key Files expanded** — Added 8 missing entries: extract-release-notes.py,
  3 shared process docs, schema reference, skill pattern template,
  marketplace.json, and GitHub Actions CI workflow.
- **Development environment docs** — Added venv, bin/ci, and dependency
  management guidance.
- **Reflect convention** — Documented that CLAUDE.md changes go through
  /reflect only.

### Fixes

- **Logging permission prompt** — Replaced Bash `>>` append (triggers
  permission prompt) with Read+Write tool pattern for completion logging.
- **Stale section removed** — Removed "What Still Needs Work" section
  containing a single speculative item.

## v0.6.2 — Test coverage hardening and permission fixes

### New Features

- **bin/ci subprocess tests** — 4 tests covering both venv and system python
  fallback paths. Uses wrapper scripts (not symlinks) for safe fixture isolation.
- **Script-coverage contract test** — `test_every_script_has_a_test_file` in
  `test_structural.py` globs `hooks/*.sh` and `bin/*` executables, fails CI if
  any script lacks a corresponding test file.
- **100% Python coverage enforcement** — pytest-cov added with `--fail-under=100`
  for all Python files in `hooks/`. Subprocess coverage routing via conftest
  session fixture.
- **Maintainer permission coverage test** — Validates every bash command in
  maintainer skills (commit, release, reflect) and shared process docs has a
  matching entry in `.claude/settings.json`.

### Fixes

- **Start phase permission prompts** — Fixed worktree and state file operations
  triggering unnecessary permission prompts.
- **Branch name length** — Capped at 32 characters, truncating at word boundaries.
- **Abort/cleanup messages** — Fixed to mention both state file and log deletion.
- **Commit temp file** — Moved from `/tmp/` to project root to avoid permission
  prompts and support concurrent sessions.
- **Maintainer permission gaps** — Added `git tag`, `git push origin`,
  `git describe`, and `git reset HEAD` to `.claude/settings.json`.
- **Bash /tmp/ references** — Contract test ensures no SKILL.md bash blocks
  reference `/tmp/` paths.

### Improvements

- **18 tests from coverage audit** — Fixed `can_return_to` drift discovered
  during audit.
- **Cross-file consistency tests** — User-facing messages validated across skills.
- **Suppressed noisy pytest output** — Header and version info hidden.
- **CLAUDE.md lessons** — 7 new lessons including symlink safety, test-first
  for bugs, fixture resource tracing, and never fabricating excuses.

---

## v0.6.1 — Documentation sync enforcement

### New Features

- **Documentation sync tests** — 13 new tests in `test_docs_sync.py` catch
  structural drift across 6 documentation layers: SKILL.md ↔ docs/skills pages
  (bidirectional), phase docs ↔ flow-phases.json (filename, command, title),
  skills index completeness, README completeness, landing page completeness,
  and state schema field coverage.
- **Commit-time docs reminder** — When SKILL.md, flow-phases.json, or the
  schema doc appear in a diff, the commit process flags `docs/` files for
  review before writing the commit message.

### Fixes

- **Logging pattern** — Fixed permission pattern matching broken by logging
  format change.
- **Release skill step numbering** — Renumbered from letter suffixes (2a, 2b)
  to clean sequential integers (1-10).
- **Permissions consolidation** — Merged settings.local.json into settings.json
  to eliminate split-file confusion.

---

## v0.6.0 — Test suite and CI pipeline

### New Features

- **48-test pytest suite** — Five test files covering the phase entry guard
  (`check-phase.py`), release notes extraction (`extract-release-notes.py`),
  session start hook (`session-start.sh`), structural invariants (phase config,
  version sync, file existence), and SKILL.md content contracts (phase gates,
  state schema, cross-references, sub-agent types, model recommendations).
- **`bin/ci` runner** — Single command to run the full test suite, with
  automatic `.venv` detection.
- **GitHub Actions CI** — Runs pytest on every push and PR to main.
- **Self-enforcing coverage** — `test_skill_contracts.py` discovers all
  `skills/*/SKILL.md` files via glob. Adding a new skill without conforming
  to conventions fails CI automatically.

### Improvements

- **CI-gated commits** — `docs/commit-process.md` now has Step 0: run `bin/ci`
  before showing the diff. Every commit in this repo is tested.
- **CI-gated releases** — `/release` now checks GitHub Actions status (Step 3)
  before proceeding. Polls up to 3 times (90 seconds) for in-progress runs.
- **Permissions expanded** — `gh run list` and `bin/ci` added to the project
  allow list.

---

## v0.5.1 — Permission prompt fixes and reflection hardening

### Fixes

- **Python heredocs replaced with tool-based checks** — All phase entry gates
  (`HARD-GATE`) now use the Read tool, Glob tool, and git commands instead of
  `python3 << 'PYCHECK'` heredocs, which failed Bash permission pattern matching.
- **`$(date)` command substitution eliminated** — All timestamp logging now uses
  `date -u +FORMAT` as the command itself instead of `echo "$(date ...)"`, which
  triggered "Command contains $() command substitution" warnings.
- **Banner setext heading rendering fixed** — All `====` banners across every
  skill are now wrapped in fenced code blocks so they render as plain monospace
  text instead of markdown H1 headings.
- **Commit message temp file scoped by repo and branch** — Prevents collisions
  between concurrent sessions across different repos and branches. Uses
  `/tmp/flow-commit-<repo>-<branch>.txt` with automatic cleanup after commit.
- **Commit process uses Write tool** — Replaced `python3 -c` file creation with
  the Write tool, avoiding shell interpretation of literal `$(...)` in commit
  message bodies. Added guidance for large diffs (use `--stat` + Read tool on
  persisted output).

### Improvements

- **Reflection self-check** — The shared reflection process now requires three
  concrete pieces of evidence for each mistake (what Claude did wrong, what the
  user said, how many correction rounds). Prevents softening mistakes in future
  reflections.
- **Three new CLAUDE.md lessons** — Always design for concurrent sessions, never
  improvise outside documented processes, read code and git history before
  proposing fixes.

---

## v0.5.0 — Shared processes, best-effort cleanup, /reflect skill

### New Features

- **`/reflect` maintainer skill** — Reviews session mistakes against CLAUDE.md
  rules and proposes targeted improvements. Uses the shared reflection process
  (`docs/reflection-process.md`) so both `/reflect` (maintainer) and
  `/flow:reflect` (Phase 7) follow the same steps.

### Improvements

- **Best-effort cleanup** — `/flow:cleanup` no longer hard-blocks when the
  state file is missing or Phase 7 is incomplete. Warns and proceeds after
  user confirmation. Infers branch and worktree from git state when the
  state file is gone.
- **Shared cleanup process** — Overlapping steps between `/flow:cleanup` and
  `/flow:abort` extracted into `docs/cleanup-process.md`. Both skills
  reference it. `/flow:abort` also softened to warn instead of block when
  the state file is missing.
- **Shared commit process** — `/commit` (maintainer) and `/flow:commit`
  now both reference `docs/commit-process.md` instead of duplicating
  commit/push/conflict-resolution logic.
- **Upgrade command in release banner** — Release completion banner now
  shows the `claude plugin marketplace update` command.
- **Session lessons captured** — CLAUDE.md updated with learnings from
  recent development mistakes (bypass /commit, safe git reset variant,
  consistency audits, verify edits against source of truth).

---

## v0.4.0 — Smart model selection, CI fix sub-agent, performance logging

### New Features

- **CI fix sub-agent in Phase 1** — When `bin/ci` fails (dirty main, RuboCop
  changes from gem upgrades, flaky tests), Phase 1 now launches a general-purpose
  Sonnet sub-agent to diagnose and fix automatically. The main Haiku agent handles
  mechanical setup at speed; Sonnet handles the reasoning when needed.
- **Model recommendations per phase** — Each phase banner now shows the recommended
  model: Opus for Design and Code (where reasoning matters most), Sonnet for
  structured phases, Haiku for mechanical steps. Commit skill recommends Sonnet.
- **Timestamp logging** — All 9 skills (8 phases + commit) now log start/done
  timestamps to `/tmp/flow-<branch>.log`. The gap between DONE and the next START
  reveals Claude's thinking time vs actual command execution.

### Improvements

- **Research scope decoupled from branch name** — Phase 2 no longer assumes what
  to research based on the feature name. The user describes what to research in
  their own words.
- **Coverage file path in CI fix instructions** — Sub-agent now reads
  `test/coverage/uncovered.txt` to know exactly which lines need coverage.
- **Expanded workspace permissions** — `bin/ci`, `rubocop`, `bundle update`,
  `bin/rails test` added to the default allow list for CI fix sub-agent.

### Docs

- README and marketing site reconciled — consistent feature example
  (`invoice pdf export`), correct Phase 1 step order, matching enforcement lists.
- Model Recommendations section added to README with rationale table.
- Sub-Agent Architecture updated to reflect Phase 1's CI fix sub-agent.
- Smart Model Selection feature card added to marketing site.

---

## v0.3.1 — Version display, commit staging fix, update command

### Improvements

- **Version shown in banners** — `/flow:start` and `/flow:status` now display
  the installed FLOW version. Hardcoded in skill files, bumped automatically by
  the release skill.
- **Commit diff uses staging** — `/flow:commit` now stages with `git add -A`
  then diffs with `git diff --cached` so new files appear in one unified diff.
  `git reset HEAD` unstages on denial (safe — just the opposite of `git add`).
- **Release skill bumps 4 files** — Version is now updated in plugin.json,
  marketplace.json, start banner, and status banner as part of every release.

### Fixes

- **Update command corrected** — README now shows the working CLI command
  (`claude plugin marketplace update flow-marketplace`) instead of the buggy
  slash command.

---

## v0.3.0 — First real-world test: bug fixes and /flow:abort

### New Features

- **`/flow:abort`** — New escape hatch skill. Abandons a feature from any
  phase: closes the PR, deletes the remote branch, removes the worktree, and
  deletes the state file. No phase gate — available whenever you need to walk
  away. Every step is best-effort so partial cleanup still works.

### Fixes

- **Start: PR creation no longer fails** — `gh pr create` was running from the
  wrong directory and GitHub rejected PRs with zero commits between base and
  head. Now creates an empty commit in the worktree before pushing and opening
  the PR.
- **Commit: new files visible in diff review** — Untracked files were invisible
  to `git diff HEAD`, forcing workarounds like `cat`. Now uses the Read tool for
  new files alongside `git diff HEAD` for tracked changes.
- **Sub-agents use proper tools** — All four sub-agent prompts (Research,
  Design, Plan, Review) now include explicit tool rules: use Glob/Read/Grep
  instead of Bash for file checks. Eliminates unnecessary permission prompts
  from `test -f` and `ls` commands.

### Improvements

- **Start step numbering cleaned up** — Old Steps 4+5 (push + PR) merged into
  a single Step 4 with all commands running from the worktree. Steps renumbered
  5-11.
- **Permissions expanded** — `gh pr close` and `git push origin --delete` added
  to the default allow list for the abort skill.

### Docs

- New docs page for `/flow:abort` with cleanup vs abort comparison table.
- Utility commands table updated in README, marketing site, and docs index.
- "Test plugin installation" removed from CLAUDE.md — tested successfully.

---

## v0.2.3 — Marketing site overhaul and commit skill fixes

### Improvements

- **Marketing site restructured** — Reorganized into What / Why / How / Get
  Started sections with a clearer narrative. "8-phase orchestration" is now
  visually emphasized as the central concept.
- **Zero Footprint section** — Added to both README and the marketing site,
  explaining that FLOW leaves nothing in your Rails project.
- **"Cool Stuff" section** — New 3D flip-card grid on the marketing site
  showcasing six standout implementation details: state persistence across
  sessions and compaction, hard phase gates that actually execute, state
  machine back-navigation, auto-generated release notes from commit history,
  self-capturing corrections, and parallel feature support via branch-named
  state files.

### Fixes

- **Commit skill message structure enforced** — Subject line, `tl;dr`, and
  per-file breakdown are now validated before display; permission prompt
  patterns corrected.
- **Commit banner rendering fixed** — Start/complete banners now render as
  plain monospace text in all markdown environments.

### Docs

- **CLAUDE.md updated** — Maintainer guidelines updated with learnings from
  recent development sessions.

---

## v0.2.2 — Repo housekeeping and maintainer tooling

### Improvements

- **Repo renamed** — `ruby-on-rails-claude-ai-process` → `flow` across all
  references, docs, and links.
- **Docs site rebuilt** — Replaced Jekyll/just-the-docs with a hand-coded
  static HTML landing page; GitHub Pages now serves `docs/index.html` directly.
- **README rewritten** — Stronger framing, deeper architecture explanation.
- **CLAUDE.md trimmed** — Removed user-facing documentation that belongs in
  README; now a concise working guide for maintainers.
- **Release skill moved to private** — `/flow:release` removed from the public
  plugin (users don't need it); now lives in `.claude/skills/release/` as a
  maintainer-only private skill invoked as `/release`.
- **`/commit` available in this repo** — Symlinked `skills/commit` into
  `.claude/skills/commit` so `/commit` works when developing in this repo
  without the plugin being self-installed.

---

## v0.2.1 — Release Skill Bug Fixes

### Fixes

- **Permission prompts eliminated** — `gh release create` was missing from the
  allow list and the `--notes` heredoc fallback used shell metacharacters. Both
  now resolved: command added to permissions, heredoc removed.
- **GitHub Release body now shows only current version** — `--notes-file
  RELEASE-NOTES.md` included all historical notes. A new
  `hooks/extract-release-notes.py` script extracts just the current version's
  section to a temp file, passed via `--notes-file` with no shell
  metacharacters.

---

## v0.2.0 — Release Skill and Sub-Agent Architecture

### New Features

- `/flow:release` — New skill for versioned plugin releases. Bumps version in
  `plugin.json` and `marketplace.json`, writes release notes, commits, tags,
  pushes, and creates a GitHub Release. Shows commits since last tag and
  recommends patch/minor/major based on commit analysis before asking for
  confirmation.

### Improvements

- **Mandatory sub-agents** — Research, Design, Plan, and Review phases now
  require Explore-type sub-agents to read the codebase. The main conversation
  stays clean for decisions; sub-agents do the reading and reporting.
- **Note capture at phase transitions** — Every phase transition (1–7) now
  offers a third option to capture a correction or learning before moving on.
- **Release skill step ordering** — Safety checks and commit list are shown
  before asking for the release type, so you see what changed before deciding.
- **`git log` always allowed** — Added `Bash(git log *)` to project permissions
  so read-only git introspection never prompts for approval.

### Fixes

- Removed Metaswarm and Superpowers phase comparison reference doc (outdated).

---

## v0.1.0 — Initial Release

The first public release of FLOW Process — an opinionated Ruby on Rails
development lifecycle plugin for Claude Code.

### What's Included

**8 Phase Skills**

Every feature follows the same phases in the same order:

1. `/flow:start` — Create worktree, upgrade gems, open PR, configure permissions
2. `/flow:research` — Explore codebase, ask clarifying questions, document findings
3. `/flow:design` — Propose 2-3 alternatives, get approval before any code
4. `/flow:plan` — Break design into ordered TDD tasks, section by section
5. `/flow:code` — TDD task by task, diff review, bin/ci gate before each commit
6. `/flow:review` — Design alignment, research risk coverage, Rails anti-pattern check
7. `/flow:reflect` — Extract learnings, update CLAUDE.md, note plugin gaps
8. `/flow:cleanup` — Remove worktree and delete state file

**4 Utility Skills**

Available at any point in the workflow:

- `/flow:commit` — Review diff, approve/deny, pull before push, commit
- `/flow:status` — Show current phase, PR link, timing, next step
- `/flow:continue` — Resume mid-session or rebuild from state on new session
- `/flow:note` — Capture corrections automatically when Claude is wrong

**Infrastructure**

- SessionStart hook — detects in-progress features, injects resume context
- Phase entry guards — prevents skipping phases
- Per-feature state files — `.flow-states/<branch>.json`
- Git rebase denied in settings
- Documentation site (GitHub Pages with Jekyll)
