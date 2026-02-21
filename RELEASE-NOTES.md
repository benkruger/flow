# Release Notes

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