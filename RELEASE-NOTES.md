# Release Notes

## v0.8.0 — One-time project setup with /flow:init

### New Features

- **`/flow:init` skill** — New utility skill that runs once after installing
  or upgrading FLOW. Configures workspace permissions in `.claude/settings.json`,
  sets up git excludes for `.flow-states/` and `.worktrees/`, writes a version
  marker to `.claude/flow.json`, and commits. Solves the chicken-and-egg problem
  where permissions written mid-session were never picked up because Claude Code
  snapshots settings at startup.
- **Version gate in `/flow:start`** — `start-setup.py` now checks
  `.claude/flow.json` before any setup work. If FLOW hasn't been initialized or
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
  `.claude/flow.json`.

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
- `/flow:resume` — Resume mid-session or rebuild from state on new session
- `/flow:note` — Capture corrections automatically when Claude is wrong

**Infrastructure**

- SessionStart hook — detects in-progress features, injects resume context
- Phase entry guards — prevents skipping phases
- Per-feature state files — `.flow-states/<branch>.json`
- Git rebase denied in settings
- Documentation site (GitHub Pages with Jekyll)
