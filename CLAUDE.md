# CLAUDE.md

A Claude Code plugin (`flow:` namespace) implementing an opinionated 8-phase development lifecycle. Supports Rails and Python via framework-specific skill fragments. Skills live in `skills/<name>/SKILL.md` with framework content in `skills/<phase>/rails.md` and `skills/<phase>/python.md`. State lives in `.flow-states/<branch>.json` in the target project.

## Key Files

- `flow-phases.json` — state machine: phase names, commands, valid back-transitions
- `skills/<name>/SKILL.md` — each skill's instructions
- `hooks/hooks.json` — SessionStart hook registration
- `hooks/session-start.sh` — detects in-progress features, injects awareness context
- `lib/check-phase.py` — reusable phase entry guard
- `.claude/settings.json` — project permissions (git rebase denied)
- `docs/` — GitHub Pages site (main /docs, static HTML)
- `lib/extract-release-notes.py` — extracts version sections from RELEASE-NOTES.md for GitHub Releases
- `lib/start-setup.py` — consolidated Start phase setup (git pull, worktree, settings, PR, state file)
- `lib/flow_utils.py` — shared utilities: `now()` (Pacific Time timestamps), `PACIFIC` timezone, `format_time()`, `current_branch()`, `project_root()`, `PHASE_NAMES`
- `lib/phase-transition.py` — phase entry/completion (timing, counters, status, formatted_time)
- `lib/set-timestamp.py` — mid-phase timestamp fields via dot-path notation
- `bin/flow` — dispatcher script routing subcommands to `lib/*.py`
- `docs/reference/flow-state-schema.md` — state file schema reference
- `docs/reference/skill-pattern.md` — template pattern for building new phase skills
- `marketplace.json` — marketplace registry (version must match plugin.json)
- `.github/workflows/ci.yml` — GitHub Actions CI (Python 3.12, pytest)

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

### Sub-Agents

Three phase skills launch mandatory sub-agents: Review and Security (general-purpose). Start uses a Sonnet sub-agent for CI failures. Plan uses Claude Code's native plan mode (`EnterPlanMode`/`ExitPlanMode`) instead of sub-agents. Code has no sub-agent. Sub-agent prompts must include a tool restriction rule and must not use Bash for file checks.

### Memory

During feature work, Claude writes auto-memory scoped to the worktree path (`~/.claude/projects/<worktree-path>/memory/MEMORY.md`). When Cleanup removes the worktree, this memory becomes orphaned. In Phase 7 mode, Reflect rescues worktree memory by reading it as a fourth source and routing useful items to permanent destinations.

Reflect is a unified tri-modal skill. It auto-detects Phase 7 (state file with Security complete), Maintainer (no state file, `flow-phases.json` exists), or Standalone (no state file, no `flow-phases.json`). All three modes edit the same 5 destinations on disk. Phase 7 adds worktree memory rescue, GitHub issues, and phase transitions. Maintainer commits via `/flow:commit --auto`. Standalone never commits.

The 5 destinations: global CLAUDE.md (`~/.claude/CLAUDE.md`), project CLAUDE.md (`CLAUDE.md` in project), global rules (`~/.claude/rules/`), project rules (`.claude/rules/` in project), project memory (`~/.claude/projects/<repo-root>/memory/MEMORY.md`). Private destinations (1, 3, 5) are written directly outside the repo. Repo destinations (2, 4) are committed via PR (Phase 7) or `/flow:commit --auto` (Maintainer). Notes captured by `/flow:note` feed into the same routing mechanism.

Commit is also tri-modal. It auto-detects FLOW (state file exists), Maintainer (no state file, `flow-phases.json` exists), or Standalone (neither). FLOW mode adds version banners and Python auto-approval. All three modes share the same diff/message/approval/push process.

### Logging

Phase skills log completion events to `.flow-states/<branch>.log` using a command-first pattern (no START timestamps). Logging goes to `.flow-states/`, never `/tmp/`.

### Version Locations

The version lives in 4 places, all must match: `plugin.json`, `marketplace.json` (top-level metadata), `marketplace.json` (plugins array entry). `test_structural.py` enforces consistency.

### State Mutations

Claude never computes timestamps, time differences, or counter increments. All standard state mutations go through `bin/flow` commands: `phase-transition` for entry/completion, `set-timestamp` for mid-phase fields. Claude still writes complex content objects (security) via Read+Write, but timestamp fields within those objects are set to null and filled separately by `set-timestamp`. The plan file lives at `~/.claude/plans/` (Claude Code's native location) and its path is stored in `state["plan_file"]`.

### Permission Invariant

Every `` ```bash `` block in every skill and docs file must run without triggering a Claude Code permission prompt. `test_permissions.py` enforces this: it extracts every bash block, substitutes placeholders with concrete values, and verifies each command matches an allow-list pattern and does not match a deny-list pattern. New bash commands require a matching permission entry. New placeholders require a `PLACEHOLDER_SUBS` entry. Unrecognized placeholders fail the test — they are never silently skipped.

## Test Architecture

Shared fixtures in `tests/conftest.py`: `git_repo` (minimal git repo), `state_dir` (flow-states dir inside git repo), `make_state()` (build state dicts), `write_state()` (write state JSON files).

| Test File | What It Enforces |
|-----------|------------------|
| `test_structural.py` | Config invariants: phases 1-8 exist, versions match across 4 files, commands unique, hooks reference existing files |
| `test_skill_contracts.py` | SKILL.md content: HARD-GATE presence, announce banners, state updates, sub-agent types, model frontmatter, logging sections, note-capture options. Uses glob-based discovery — new skills are automatically covered |
| `test_check_phase.py` | Phase guard: blocks on incomplete prerequisites, allows on complete, handles worktrees, re-entry notes |
| `test_session_start.py` | Session hook: feature detection, timing reset, awareness injection, multi-feature handling |
| `test_docs_sync.py` | Docs completeness: every skill has a docs page, every phase has a docs page, index and README mention all commands |
| `test_permissions.py` | Permission simulation: allow/deny coverage, placeholder validation, source-of-truth sync between init-setup.py and init/SKILL.md, regex unit tests. Unrecognized placeholders fail loudly |
| `test_bin_ci.py` | CI runner: venv detection, pass/fail behavior |
| `test_bin_test.py` | Test runner: venv detection, pass/fail, argument passthrough |
| `test_start_setup.py` | Start setup script: branch naming, settings merge, worktree, state file, logging, error paths |
| `test_phase_transition.py` | Phase entry/completion: timing, counters, status, formatted_time |
| `test_set_timestamp.py` | Mid-phase timestamps: dot-path navigation, NOW replacement |
| `test_extract_release.py` | Release notes extraction from RELEASE-NOTES.md |

## Maintainer Skills (private to this repo)

- `/qa` — `.claude/skills/qa/SKILL.md` — `--start`/`--stop` dev mode with cache nuke, swap marketplace source
- `/release` — `.claude/skills/release/SKILL.md` — bump version, tag, push, create GitHub Release
- `/reset` — `.claude/skills/reset/SKILL.md` — remove all FLOW artifacts (worktrees, branches, PRs, state files)

## Conventions

- All commits via `/flow:commit` skill — no exceptions, no shortcuts, no "just this once"
- All changes require `bin/flow ci` green before committing — tests are the gate
- New skills are automatically covered by test_skill_contracts.py (glob-based discovery)
- Namespace is `flow:` — plugin.json name is `"flow"`
- Never rebase — merge only (denied in `.claude/settings.json`)
- CLAUDE.md changes only through `/reflect` — never edit CLAUDE.md directly. The `/reflect` skill exists to review mistakes, propose additions, get individual approval for each change, and commit. Editing CLAUDE.md outside of `/reflect` bypasses all of that.
- **Never add pymarkdown exclusions** — The `.pymarkdown.yml` disables MD013 (line length), MD025 (multiple H1 with frontmatter), MD033 (inline HTML), and MD036 (emphasis as heading) because those conflict with this repo's intentional patterns. No further rule disablements or path exclusions may be added. If a markdown file triggers a lint error, fix the file — do not suppress the rule. If a rule genuinely cannot be satisfied, surface it to the user for a decision.
- **Skills must never instruct Claude to compute values** — no timestamp generation, no time arithmetic, no counter increments, no `date -u`. All computation goes through `bin/flow` subcommands. Skills say "run this command", never "calculate this value". `test_skill_contracts.py` enforces this: `test_phase_skills_no_inline_time_computation` fails if any phase skill contains computational instruction patterns.
- **All timestamps use Pacific Time** — `lib/flow_utils.py` provides `now()` which returns `datetime.now(ZoneInfo("America/Los_Angeles")).isoformat(timespec="seconds")`. All scripts import `now` from `flow_utils` — never generate timestamps locally. Existing state files with UTC timestamps (`Z` suffix) are handled by `datetime.fromisoformat()` which parses both formats.
- **Prefer dedicated tools over Bash for all non-execution tasks** — Read files with the Read tool, search with Glob and Grep, create with Write, modify with Edit. Bash should only be used for commands that genuinely require shell execution: `bin/ci`, `bin/test`, `bin/flow`, `make`, and `git`. In this project's strict permission environment (`defaultMode: "plan"`), every Bash command not in the allow list triggers a permission prompt. When you need to explore, understand, or modify files, use dedicated tools — they never prompt.

## Lessons Learned

- **Never bypass `/flow:commit`** — even when the change is small, even when you just used it two commits ago. "All commits via `/flow:commit` skill" is not a guideline, it is a rule. The user had to interrupt mid-commit to stop this.
- **When fixing mistakes, propose the safe variant first** — `git reset --soft` is safe (keeps changes staged). Bare `git reset` is forbidden. Always specify `--soft` and explain why it's non-destructive before asking permission.
- **Consistency audits require comparing the canonical source first** — When reconciling README and docs, start by identifying the canonical example (the marketing page) and grep for every divergence. Do not edit piecemeal and hope you caught everything. The most obvious inconsistency (different feature example names across files) was the one missed.
- **Verify edits against the source of truth before saving** — When fixing an ordering issue, re-read the SKILL.md to confirm the correct order before writing the edit. Editing from memory introduced the exact error being fixed.
- **Always design for concurrent sessions** — Multiple FLOW features can run simultaneously in different worktrees. Any fix involving shared resources (temp files, log files, state) must be scoped by repo and branch. A fixed filename like `/tmp/flow_commit_msg.txt` will be clobbered by parallel sessions. Always ask: what happens if two sessions hit this at the same time?
- **Never improvise outside documented processes** — When the commit process didn't cover large diffs, Claude improvised a shell redirect to `/tmp/` which triggered a permission prompt. The right answer was already available: `git diff --cached --stat` for summaries, and the Read tool on the Bash tool's persisted output file. If a documented process doesn't handle your situation, propose a process change — don't work around it.
- **When shown a bug, read the code and git history before proposing a fix** — When the user reports a bug (especially with screenshots), read the affected files, run `git log` and `git blame` to understand when and why the current code was written, then trace the actual mechanism before suggesting anything. Guessing at fixes without reading the code or history led to three wrong proposals in a row. The global CLAUDE.md rule applies here too: STOP, READ, INVESTIGATE, UNDERSTAND, REPORT, ACT.
- **When inserting a step into a numbered sequence, renumber all subsequent steps** — Never use letter suffixes (2a, 2b) or fractional numbering. Maintain clean sequential integers and update all internal cross-references to the renumbered steps.
- **Test-first for bug fixes** — When a bug is found, write a failing test that reproduces the bug before writing the fix. The failing test proves the bug exists; the fix makes it pass. Do not fix first and add tests afterwards — that inverts the feedback loop and risks writing tests that pass by coincidence.
- **When a fix fails, stop and re-diagnose before pivoting** — If a committed fix doesn't solve the problem, do not immediately start coding a different approach. Stop, explain why the fix failed (or say you don't know), present the new diagnosis and proposed approach, and wait for approval. The session protocol applies every time, not just the first time.
- **Test permission changes before committing** — When adding or changing permission patterns in settings.json, verify the format is valid and the pattern will actually be honored by Claude Code. If you cannot verify (e.g., unsure whether Write(...) is a valid permission type, or whether out-of-project paths are covered), say so and propose a testable alternative rather than committing an unverified fix.
- **Answer completely when asked to explain something** — When the user asks "why does X do Y" or "audit this file", explain every item, not just the interesting ones. Do not dismiss entries as "straightforward" or skip them. If you don't explain it, the user has to ask again.
- **Only gitignore files that exist or will be created in this repo** — Do not add preventive gitignore entries for files that might theoretically appear. If a file does not exist and no process in this repo creates it, it does not belong in .gitignore. Gitignore entries for nonexistent files are clutter.
- **Never fabricate excuses for mistakes** — When you make a mistake, state what you did wrong and stop. Do not invent explanations like "time pressure" or "complexity" to soften it. There is no time pressure. Say what happened, not why it was understandable.
- **Never create symlinks to real binaries in test fixtures** — `Path.write_text()` follows symlinks and overwrites the target. A symlink to `sys.executable` + `write_text()` will corrupt the actual python binary. Use wrapper scripts (`exec <real_path> "$@"`) instead of symlinks when tests need a fake executable at a known path.
- **Trace every fixture operation that touches real system resources** — When a test fixture creates references to real files, binaries, or executables (symlinks, paths from sys.executable, shutil.copy), mentally trace every subsequent operation in the test. If any operation (write_text, open, chmod) could follow a reference back to the real resource and mutate it, the fixture is unsafe. Replace indirect references with self-contained fakes (wrapper scripts, copied binaries) that cannot escape the temp directory.
- **Never use `bypassPermissions` mode on sub-agents** — Permission deny lists exist to prevent destructive operations like `git stash`, `git rebase`, and force-push. Setting `mode: "bypassPermissions"` on a Task agent disables all permission checks, letting the agent run forbidden commands. Always use the default mode. If a sub-agent needs a permission that is denied, surface it to the user rather than bypassing the deny list.
- **Test-first for all changes, not just bug fixes** — When adding or modifying any skill or hook behavior that has test coverage in test_skill_contracts.py or other test files, write the failing test first, then make the change to pass it. TDD is not limited to bug fixes — any change that should be enforced by a test must start with the test.
- **Present a plan before multi-file changes** — When a change touches more than 2-3 files, list every file that will change and describe the change before editing anything. Wait for explicit approval. The global CLAUDE.md session protocol ("Propose plan, list files to change, wait for approval") applies here with no exceptions.
- **Before changing a command or string in any file, grep the full codebase for all occurrences first** — Skills, docs, and tests can all reference the same command. Changing it in one place without checking creates drift that tests will catch but you should have prevented. Run a codebase-wide search before making targeted edits.
- **Scope fixes to the original bug** — A fix for a narrow problem (e.g., permission prompt on state file writes) must not change project-wide behavior (e.g., setting defaultMode for all Write/Edit operations). If the targeted fix is impossible, say so and propose alternatives rather than widening the blast radius.
- **Never suggest removing safety checks** — The baseline `bin/ci` in Start Step 8 exists to distinguish pre-existing failures from gem-upgrade failures. Do not suggest removing, skipping, or making optional any safety check that gates a phase transition. If performance is a concern, propose making it faster, not removing it.
- **When bin/ci reveals an unexpected conflicting test, report before fixing** — If the plan says to change files A and B but bin/ci fails because file C enforces the old behavior, stop and tell the user: name the conflicting test, explain why it conflicts, and describe the fix. Do not silently expand the scope. The fix may be obvious, but the user should know the plan grew.
- **Commit subjects describe the goal, not the mechanism** — When a change has both a mechanism (what was done) and a goal (why it was done), the subject line must describe the goal. "Move scripts from hooks/ to lib/" is mechanism. "Consolidate 7 permission entries into 1" is the goal. The body explains the mechanism — the subject line tells readers why they should care.
- **Check test and build configuration when moving source files** — When moving files to a new directory, grep for the old directory path in configuration files too: pytest.ini, Makefile, CI configs, coverage settings. These files reference source directories but are easy to miss because they are not code and not docs.
- **Never claim something doesn't exist without searching first** — When the user asks about a skill, file, or feature, search the codebase (Glob or Grep) before answering. Claude claimed flow:init didn't exist because it wasn't in the 8 numbered phases, but skills/init/SKILL.md was right there. Memory is not a substitute for a file search.
- **Update CLAUDE.md when the project's scope changes** — When a feature changes what the project is (e.g., single-framework to multi-framework), update CLAUDE.md's project description and architecture sections in the same change. Stale descriptions mislead future sessions.
- **Never use replace_all=True on JSON state file edits when the old_string appears in multiple semantic contexts** — "pending" appears in both task statuses and phase statuses. Use targeted old_string with enough surrounding context to make the match unique to a single location.
- **Store numeric state fields as raw integers, never formatted strings** — `cumulative_seconds` and `visit_count` must be integers in the JSON state file. The human-readable format (e.g. `"<1m"`, `"5m"`) is for display output only and must never be written to storage.
- **Before changing output formats, grep tests for the old format** — When switching timestamps from UTC to Pacific Time, 5 tests failed because they asserted UTC-specific patterns (`endswith("Z")`, regex requiring `Z$`). Before changing any value format in production code, search test files for assertions that depend on the old format and update them in the same change.
- **Never skip `git add -A` in flow:commit Step 1** — The Code phase task review shows diffs via `git diff HEAD`, which displays unstaged changes without staging them. The commit skill must always run `git add -A` before `git diff --cached`, regardless of whether a diff was shown earlier in the same task.
- **When a skill says "for each X, do Y", process every item** — During /reset, the inventory found 2 worktrees, 4 branches, and 3 state files, but only 1 of each was actually deleted. The report then claimed all items were handled. Always iterate through the full list, run the command for each item, and count actual operations — never report counts that don't match commands run.
- **When changing a behavior, search for all descriptions of that behavior — not just exact string matches** — Grepping for the specific command or string being changed catches direct references but misses summary sections (like a "Gates" list in docs) that describe the same behavior using different wording. When a behavior changes, scan every file that documents or summarizes it for semantic descriptions, not just textual matches.
- **Never run `git add -A` in commit Step 4** — Files are already staged from Step 1. Running `git add -A` again in Step 4 stages `.flow-commit-msg` itself (just written by the Write tool), causing it to be tracked in the commit. Run only `git commit -F .flow-commit-msg` in Step 4.
