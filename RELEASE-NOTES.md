# Release Notes

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
- Per-feature state files — `.claude/flow-states/<branch>.json`
- Git rebase denied in settings
- Documentation site (GitHub Pages with Jekyll)