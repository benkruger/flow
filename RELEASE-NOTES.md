# Release Notes

## v0.33.1 — Concurrency awareness and safety guards

### Improvements

- Add concurrency context to all 12 phase and auxiliary skills — establishes N×N×N model (state isolation, branch scoping, GitHub idempotency) at runtime
- Add "never discard uncommitted changes" safety guard to flow-complete, flow-commit, and flow-code-review
- Split concurrency-model.md — architecture principles moved to CLAUDE.md, developer checklists stay as rule
- Code Review plugin mode now configurable via `code_review_plugin` axis ("always", "auto", "never")
- Sub-skill continuation hardened — stop-continue hook enforces self-invocation loops across built-in skill boundaries
- Skill authoring rules expanded — negative-assertion test compatibility, codebase-wide renames, config chain integrity
- Strengthen skill instructions with explicit hard rules and gate enforcement

### Fixes

- Doc drift schema issues fixed

## v0.33.0 — Complete phase improvements, flow_utils modernization, and phase transition enforcement

### New Features

- Business-friendly Done banner in Complete phase via `format-complete-summary` — shows feature name, prompt, per-phase timeline, and artifact counts
- HARD-GATE enforcement on all phase skill Done sections — prevents auto-momentum from skipping `continue=manual` transition prompts

### Improvements

- Shared `detect_repo()` and `mutate_state()` in flow_utils — consolidates duplicated repo detection regex and adds fcntl file locking for atomic state writes
- Subprocess timeouts (30s) on all `gh` CLI calls — prevents indefinite hangs on network issues
- `extract_issue_numbers()` now matches GitHub issue URLs (`/issues/N`) in addition to `#N` patterns
- Start prompt stored verbatim in state file via `--prompt` flag
- PR body rendering deduplicated into single `render-pr-body` call

### Fixes

- Complete skill: premature `cd` to project root broke Step 3 merge and phase-transition with concurrent flows (#241)
- Complete skill: MERGED fast-path now continues through cleanup (Steps 10-11) instead of terminating at Step 9
- PR body fenced blocks: fixed pymarkdown MD031 violations in rendered sections
- Session-start tests: fixed to use branch field instead of removed feature field
- CI fixture: set default branch to main for GitHub Actions compatibility

## v0.32.4 — Fix hook isolation for concurrent flows

### Fixes

- Fix SessionStart hook branch isolation — filter state files to current branch only, preventing wrong-context injection and timing data corruption when multiple flows are active on the same machine. Fail-open fallback on detached HEAD.
- Fix stop-continue hook session and branch isolation — replace resolve_branch() with current_branch() for exact match, reorder main() to check before capture for stale flag detection, add session_id comparison to clear flags from previous sessions.

## v0.32.3 — Fixes and documentation

### Fixes

- Fix Read(/tmp/) permission patterns to use double-slash absolute paths.
- Remove the Learn-phase-only restriction on CLAUDE.md edits because it was blocking legitimate edits in other phases.

### Improvements

- Add phase-transition calls to Complete so its timing appears in Phase Timings.
- Use the full start prompt for the PR "What" section because the title-cased slug loses the user's actual description.
- Add N×N×N concurrency as the 4th core design tenet because past bugs stemmed from assuming single-flow usage.

## v0.32.2 — Fixes and improvements

### Fixes

- Fix release skill staging order to prevent `.flow-commit-msg` from being committed.
- Fix CI dirty check to use default branch detection.

### Improvements

- Flow issues now filters WIP items using the "Flow In-Progress" label.
- PR artifacts: improved section rendering and content handling.
- Complete phase banner now includes the feature name.

## v0.32.1 — Fix Code Review dropping background agent findings

### Fixes

- Code Review steps now wait for all background agents to complete before
  evaluating findings. Previously, built-in skills (/simplify, /review,
  /security-review) launched background agents whose findings were silently
  dropped because the stop-continue hook resumed the parent skill before
  agents completed. Closes #175, #209.

### Housekeeping

- Remove stale .flow-commit-msg left over from the v0.32.0 release.

## v0.32.0 — Start lock wait flag and bug fixes

### New features

- Start lock `--wait` flag: callers can now wait for an existing lock to release instead of failing immediately (#202)

### Fixes

- Learn phase in manual mode no longer stops prematurely (#204)
- Code Review phase timing no longer shows 0 seconds (#200)
- Finalize-commit handles rev-parse and timeouts correctly (#201)
- Allow reading /tmp diff files for commit review (#203)
- Phase timing table no longer shows stale "or" formatting (#195)
- Complete banner formatting improved (#196)
- Flow commit message file cleanup after finalize (#198)
- Code Review Step 1 (Simplify) invocation fixed (#199)

### Improvements

- CLAUDE.md test architecture table added and CI-enforced (#206)

## v0.31.4 — Fix flow-start mode resolution and CI gate

### Fixes

- flow-start mode resolution now reads from the state file in the Done section
  instead of .flow.json directly — fixes auto mode falling through to manual
  after cd into worktree. Closes #174, #187.
- flow-start CI baseline gate on main before worktree creation. Closes #188.

## v0.31.3 — Fix flow-start existing-feature check and Complete phase continuation

### Fixes

- Remove existing-feature check from flow-start — flow-start no longer scans
  .flow-states/ for active features or prompts about them. Multiple simultaneous
  features are supported by design; flow-continue is the command for resuming.
  Closes #184.
- Fix Complete phase stopping after merge conflict resolution because
  _continue_pending was not set before commit. Closes #178.

## v0.31.2 — PR enrichment, issue batching, and commit enforcement

### Improvements

- Complete phase done banner now includes PR link and phase timing summary
- PR body includes key sections (plan, timing, issues filed)
- Issues skill supports batching related issues into a single filing

### Fixes

- Enforce per-task commits and self-invocation loops via preset
- Add problem-vs-solution boundary rule to filing-issues skill
- Fix batch-of-related-issues routing
- Untrack scheduled_tasks.lock (session-specific state, not repo content)
- Clean up stale commit message file from v0.31.1 release

## v0.31.1 — Fixes and cleanup

- Remove the version-bump-on-hash-change rule from CLAUDE.md — version bumps
  are now exclusively a release decision via /flow-release, not a side effect
  of permission changes (#157, closes #156)
- Fix cleanup to delete plan and DAG files left behind by the Complete phase
- Add worktree bin/flow rule for repo-modifying commands during Code phase
- Fix iOS framework permission issue (#153)
- Add ci-fixer agent permission and clean up stale commit message file handling

## v0.30.0 — DAG-enhanced planning via decompose plugin

- Plan phase now invokes the `decompose` plugin (`decompose:decompose`)
  for structured DAG analysis — nodes, dependencies, topological ordering
- Plan and DAG files stored in `.flow-states/` alongside other feature
  artifacts, embedded as collapsible sections in the PR body
- DAG mode configurable via `.flow.json` (`auto`/`always`/`never`)
- `flow-prime` installs the decompose plugin automatically
- Removed Claude Code native plan mode (`EnterPlanMode`/`ExitPlanMode`)
  — replaced by direct decompose plugin invocation
- Removed `lib/validate-exit-plan.py` hook — no longer needed
- Content standards added to issue filing rules
- Design document at `docs/reference/dag-planning-design.md`

## v0.29.0 — iOS framework support

- Add iOS as third supported framework alongside Rails and Python
- Data-driven detection via `*.xcodeproj` glob pattern
- iOS permissions: `open`, `xcrun`, `xcodebuild`
- iOS priming: SwiftUI, Swift Testing, structured concurrency conventions
- iOS dependency template: SPM resolution when Package.swift exists
- Enable glob-based detection in detect-framework.py (backwards-compatible)
- Add derived permissions: iOS auto-derives `killall AppName` from xcodeproj name
- Widen rm permission from `.flow-commit-*` to `.flow-*`

## v0.28.22 — PostCompact hook, permission cleanup, and fast reprime

- Add PostCompact hook to capture conversation summary after context
  compaction (#135). State file stores compact_summary, compact_cwd,
  and compact_count for SessionStart to inject on resume.
- Add --reprime flag to /flow-prime for fast upgrades that reuse
  existing config instead of re-running full setup.
- Consolidate 7 gh issue permission entries into 1 wildcard for
  easier auditing.
- Replace --body with --body-file in issue filing to avoid Bash hook
  validator failures on special characters.
- Add Agent(flow:ci-fixer) to permissions and constrain ci-fixer to
  project directory.

## v0.28.21 — Fix Ruby CI scripts and require release approval

### Fixes

- `ci.py` no longer hardcodes `bash` as the interpreter for the target project's `bin/ci`.
  Scripts with shebangs (Ruby, Python, etc.) now run correctly via the OS interpreter.

### Improvements

- `/flow-release` now defaults to manual approval. `--auto` flag required to skip the prompt.
- New `target_project` test fixture simulates a non-bash target project (Python `bin/ci`, no
  `bin/flow`) so integration tests catch interpreter and path assumptions that the FLOW repo masks.
- New "Target Project Mindset" rule in skill-authoring guidelines.

## v0.28.20 — Fix bin/flow resolution in target projects

### Fixes

- All `bin/flow` calls in plugin skill bash blocks now use `exec ${CLAUDE_PLUGIN_ROOT}/bin/flow`.
  Bare `bin/flow` resolved locally during plugin development but failed with exit 127 in every
  target project.
- New CI-enforced contract test (`test_plugin_skills_use_plugin_root_for_bin_flow`) prevents
  bare `bin/flow` from being reintroduced in any plugin skill.

### Improvements

- **flow-release**: 19 sequential rounds → 11 via parallelization and `finalize-commit` reuse.

## v0.28.19 — Skill performance: parallelization and round-trip reduction

### Improvements

- **flow-commit**: 12 sequential rounds → 6. New `finalize-commit.py` script consolidates commit + cleanup + pull + push into one call. Mode detection and format resolution parallelized with Glob. CI and staging run in parallel. Status and diff run in parallel.
- **flow-start**: 18 sequential rounds → 9. Version gate, upgrade check, existing feature check, and CI all run in one parallel round. New `log.py` subcommand replaces the 2-round Read+Write logging pattern with a single call. Log entries pipelined with the next command.
- **flow-prime**: detect-framework and `claude plugin list` run in parallel in Step 1, cached result reused in Step 5.

## v0.28.18 — Fix auto-upgrade skipping artifact reinstallation

### Fixes

- Auto-upgrade now checks both `config_hash` (permission structure) and
  `setup_hash` (installer file content). Previously, changes to installed
  artifacts like the pre-commit hook were silently skipped because only
  `config_hash` was compared. Projects upgrading to this version will be
  prompted to re-run `/flow:flow-prime`.
- Removed `SETUP_EPOCH` — redundant now that `setup_hash` covers all
  changes to `prime-setup.py`.

## v0.28.17 — Model-agnostic skills and consolidated git permissions

### Improvements

- Remove `model:` frontmatter from all skills and the ci-fixer agent so FLOW
  inherits the user's session model — no plugin changes needed when models evolve.
- Consolidate 18 specific `Bash(git ...)` allow entries into one `Bash(git *)` so
  unanticipated git commands (e.g. `git rev-parse`, `git show`, `git blame`) are
  never blocked by the PreToolUse hook. The deny list still blocks destructive
  operations.

### Cleanup

- Remove Model column from phase tables in README.md and CLAUDE.md.
- Remove "Model Recommendations" section from README.md.
- Delete `test_model_frontmatter_is_valid` and
  `test_model_frontmatter_matches_documented_table` tests.
- Remove git entries from `AUTO_ALLOWED` in test_permissions.py (now covered by the
  allow list).

## v0.28.16 — Scope pre-commit hook to active FLOW features

### Fixes

- Pre-commit hook no longer blocks command-line commits when no FLOW feature is
  in progress. The hook now checks for a `.flow-states/<branch>.json` state file
  before blocking — commits outside active FLOW features pass through normally.
- Fixed test that hardcoded `main` as the branch name, failing on CI where the
  default branch is `master`.

## v0.28.15 — Faster flow-prime setup

### Improvements

- Consolidate flow-prime Steps 4–8 into a single `bin/flow prime-setup` call,
  reducing tool calls from ~10 to ~6. The script now accepts `--skills-json`
  and `--commit-format` args to write the complete `.flow.json` in one pass,
  and calls prime-project and create-dependencies internally.

## v0.28.14 — Fix ci-fixer commit enforcement and docs accuracy

- Start Phase: ci-fixer fixes on main now use `/flow:flow-commit --auto` with a
  HARD-GATE to prevent uncommitted changes from being invisible in the worktree.
  Step 6 dependency commits also use `--auto` for consistency.
- Commit skill: `--auto` exception list expanded from just `/flow:flow-learn` to
  all phase skills that commit autonomously.
- Docs: Removed incorrect claim that FLOW commits files to the repo — nothing is
  committed.

## v0.28.13 — Docs restructure and auto-loop CI pending

- Restructured README and marketing site around three core goals (Unobtrusive,
  Autonomous or Manual, Safe for Local Env) with dedicated Guardrails, Autonomy,
  and Commands sections
- Fixed Code Review description from "three lenses" to "four lenses" across
  docs/skills/index.md, README, and index.html
- Start and Complete now auto-invoke `/loop` when CI is pending instead of
  stopping and telling the user to do it

## v0.28.12 — System-enforce auto-continue via PreToolUse hook

- **AskUserQuestion hook enforcement** — auto-continue (`continue=auto`) is
  now enforced by a PreToolUse hook on AskUserQuestion, preventing the model
  from prompting when autonomous phase transitions are configured. Previously
  relied on instruction-based enforcement which the model forgot after many
  tool calls.
- **State file flag** — `phase_complete()` sets `_auto_continue` in the state
  file when the current phase has `continue=auto`; `phase_enter()` clears it
  on next phase entry. Handles both object and flat string skill configs.
- **Start model reverted to haiku** — the sonnet upgrade in v0.28.11 was a
  workaround for mode retention, no longer needed with hook enforcement.

## v0.28.11 — Fix auto-continue and cleanup artifact bugs

- **Start skill upgraded to sonnet** — haiku lost track of the resolved
  continue mode after intermediate skill invocations, causing prompts even
  in auto mode. Sonnet retains state across Skill tool boundaries.
- **Start Done reordered for auto mode** — checks continue=auto before
  invoking flow:flow-status, skipping both status display and
  AskUserQuestion when running autonomously.
- **Plan completion before ExitPlanMode** — phase-transition complete now
  runs in Step 3 before ExitPlanMode, so the state is always updated even
  when ExitPlanMode clears context. Done section is now best-effort.
- **Session-start hook detects never-entered phases** — when current_phase
  was advanced but phase-transition enter never ran (status still pending),
  the hook now triggers auto-continue instead of waiting for user input.
- **Complete phase cleanup fix** — .flow-states/ timing artifact files no
  longer left behind after phase completion.

## v0.28.10 — Safety and permission fixes

- **Block git restore .** — PreToolUse hook now blocks blanket `git restore .`
  to prevent silent loss of uncommitted changes. Code Review skill updated to
  use per-file restores instead.
- **Restore Read(/tmp/*.txt) permission** — recovered permission entry that was
  accidentally lost during a Complete phase cleanup.
- **Checksum → version invariant** — structural test enforcing that config_hash
  changes require version bumps. ci-fixer now uses `rubocop -A` for RuboCop
  violations.
- **Issue filing improvements** — `bin/flow issue` and `bin/flow add-issue`
  for tracking issues filed during Learn and Code Review phases.
- **Settings.json runtime whitelist** — PreToolUse hook enforces the allow list
  as a whitelist, blocking commands not matching any pattern.

## v0.29.0 — Issue-driven autonomous workflow

Replace direct `.claude/rules/` edits with GitHub issues to keep the autonomous cycle unbroken. Five issue types filed across three phases, all tracked in the state file and surfaced at Complete.

### Issue types

| Label | Filed during | What it captures |
|-------|-------------|-----------------|
| Rule | Learn | Rule additions/updates for `.claude/rules/` — deferred to a future session |
| Flow | Learn | FLOW process gaps (previously labeled `learning`) |
| Flaky Test | Code | Intermittent test failures with reproduction data |
| Tech Debt | Code Review | Pre-existing, out-of-scope code quality issues |
| Documentation Drift | Code Review, Learn | Docs out of sync with actual behavior |

### Changes

- Learn Step 3: `.claude/rules/` edits now filed as "Rule" issues instead of editing files directly
- Learn Step 6: label changed from `learning` to `Flow`, added Documentation Drift issue filing
- Code CI Gate: flaky test detection — files "Flaky Test" issue on retry-pass
- Code Review: out-of-scope findings classified as Tech Debt or Documentation Drift and filed as issues
- Complete Step 6: "Issues Filed" table added to PR body when issues exist
- Complete Done banner: shows issues count breakdown when issues were filed
- flow-issues skill: five label-based categories (Rule, Flow, Flaky Test, Tech Debt, Documentation Drift) with Bug/Enhancement/Other as content-based fallbacks
- New `lib/add-issue.py`: records filed issues in state file `issues_filed` array
- New `bin/flow add-issue` subcommand
- State schema: added `issues_filed` array

## v0.28.9 — Runtime whitelist enforcement

- PreToolUse hook now enforces .claude/settings.json allow list as a whitelist — commands not matching any Bash(...) pattern are blocked with exit 2
- Extracted permission_to_regex() into flow_utils.py for shared use between hook and tests
- Added read-only git patterns (status, diff, log, branch) and cd to prime allow list
- Simplified bin/ permissions: bin/ci + bin/dependencies + bin/test replaced with bin/* glob

## v0.28.8 — Bug fixes for session log artifact, Plan autonomy, and issue closing

### Fixes

- Move session log artifact from Plan Step 3 to Complete Step 6 — the conditional
  in Plan was unreliable (worked in #106, silently dropped in #113). Complete already
  has the state file loaded and transcript_path is guaranteed populated by Phase 6.
- Add flow-plan to autonomy presets so the Plan phase respects continue=auto from
  .flow.json instead of always defaulting to manual.
- Fix missing close-issues call in bin/flow dispatcher.

## v0.28.7 — Bug fixes for phase halting and ci-fixer agent name

### Fixes

- Fix ci-fixer sub-agent launch failure — skills now use the namespaced
  `flow:ci-fixer` agent type instead of bare `ci-fixer`, eliminating the
  wasted "agent not found" error on every CI failure
- Fix Code and Learn phases halting after child skill returns — applied
  self-invocation pattern so phases auto-resume after `/flow:flow-commit`
  and other built-in skills return control
- Add `--auto` and `--manual` flags to `/flow:flow-commit` so phase skills
  can control approval behavior explicitly

## v0.28.6 — Worktree path safety hook

- Add PreToolUse hook (`validate-worktree-paths.py`) that blocks Edit, Write,
  Read, Glob, and Grep calls targeting the main repo path when working inside a
  FLOW worktree — prevents edits from accidentally landing on main instead of
  the feature branch
- Add `gh issue view` permission to settings.json so flow-plan can fetch
  referenced GitHub issues during planning

## v0.28.5 — Plan phase fetches referenced GitHub issues

- Plan Step 1 now scans the prompt for `#N` patterns and fetches each
  referenced issue via `gh issue view` before exploration begins. The
  issue body becomes primary planning context instead of inferring from
  the prompt words alone.
- Added `gh issue view *` permission to prime-setup and flow-prime reference.
- Contract test enforces the instruction persists.

## v0.28.4 — ExitPlanMode safety hook

### Fixes

- Add PreToolUse hook on ExitPlanMode that blocks exiting plan mode until
  `plan_file` is stored in the state file. Prevents context compaction from
  losing the plan location, which caused worktree confusion and bad commits
  to main during #107.

## v0.28.3 — Fix start phase blockers

### Fixes

- Start-setup initial commit now writes `.flow-commit-msg` before committing,
  matching the pre-commit hook's expected fingerprint file. Previously, the
  hook blocked all new features from starting (#108).
- Feature name HARD-GATE no longer prompts interactively when arguments are
  provided. Models now pass all argument words through verbatim instead of
  filtering or re-asking. Missing arguments show a usage error instead of
  an AskUserQuestion prompt (#109).

## v0.28.2 — Bug fixes for prompt storage, permissions, and plan autonomy

### Fixes

- Preserve raw start prompt in state file via `--prompt` flag, so `#N` issue
  references survive branch-name sanitization and issues close correctly at
  completion.
- Add `Read(~/.claude/rules/*)` permission so code-review plugin sub-agents
  can read user rules without prompting.
- Add Mode Resolution to flow-plan skill so `continue=auto` in `.flow.json`
  auto-advances Plan→Code without prompting.
- Fix pre-commit hook blocking commits when `.flow-commit-msg` is missing
  (#106).

## v0.28.1 — Bug fixes

### Fixes

- Fix session log artifact missing from PRs by combining both artifact commands into one
- Replace sleep+retry CI polling in release skill with /loop suggestion to avoid blocking the session

## v0.28.0 — Close issues in the Complete phase

### New

- **Issue auto-close**: Complete phase now closes GitHub issues referenced
  in the `/flow-start` prompt (`#N` patterns) after merge via `gh issue close`
- **Prompt-driven planning**: Plan phase reads the start prompt directly from
  the state file instead of asking "What are we building?" — seamless
  Start→Plan transition
- **New state field**: `prompt` stores the full `/flow-start` text for use by
  Plan and Complete
- **New script**: `lib/close-issues.py` extracts issue references and closes
  them with best-effort error handling

### Improvements

- **Branch name sanitization**: Special characters (`#`, `@`, `$`) are stripped
  from branch names derived from the start prompt
- **Skill authoring rule**: Skip/jump targets must be audited for intent when
  inserting new steps (not just mechanically incremented)

## v0.27.2 — Fix Stop hook permission denied

### Fixes

- Fix Stop hook `Permission denied` error — `stop-continue.py` was missing
  execute permission since v0.27.0.

### Improvements

- Add `test_hook_scripts_are_executable` structural test to catch missing +x
  on all hook scripts.

## v0.27.1 — Fix session_id capture to use Stop hook stdin

### Fixes

- `session_id` and `transcript_path` in the state file were always `null`
  because the previous implementation read from `CLAUDE_SESSION_ID` env var,
  which doesn't exist in Claude Code. The Stop hook now captures both fields
  from hook stdin JSON on every model response, writing them into the active
  state file. Idempotent — skips the write if `session_id` is already set.
- Session log artifact moved from Complete Step 6 to Plan Step 3, where
  `transcript_path` is already available from the state file.

## v0.27.0 — Stop hook fixes phase halting after child skill invocations

### New

- **Stop hook** (`lib/stop-continue.py`) forces the model to continue after
  child skills (/simplify, /review, /security-review, code-review:code-review,
  /flow:flow-commit) return. Uses a `_continue_pending` flag in the state
  file — set before invoking, cleared by the hook when it blocks the stop.
  Fail-open design ensures normal operation is never disrupted.

### Fixes

- Code Review no longer halts between steps when a child skill returns
  (#86, #87, #88, #96, #97, #98).
- Code phase no longer halts after /flow:flow-commit returns between
  tasks (#101).

## v0.26.0 — Repo-only Learn phase

- Learn phase reduced from 5 destinations to 2 repo-local destinations
  (Project CLAUDE.md and project rules). Eliminates all writes to
  `~/.claude/` paths, removing unavoidable permission prompts that
  fired every session.
- New contract tests enforce repo-only routing — private paths are
  permanently blocked.
- New `skill-authoring.md` rule: grep for old numbers when renumbering
  destinations.

## v0.25.0 — PR body archives phase timings and session link

### New features

- **Phase timings table** — Complete (Phase 6) now generates a visible, non-collapsible
  markdown table in the PR body showing how long each phase took.
- **Session log link** — Complete adds a session transcript artifact to the PR body
  when `session_id` is available in the state file.
- **`--no-collapse` mode** — `update-pr-body --append-section` can now render plain
  markdown sections instead of collapsible `<details>` blocks.

### Fixes

- **Session log slug bug** — Removed `.lstrip("-")` from slug computation in
  `start-setup.py` so the path matches Claude Code's actual directory naming
  (e.g. `-Users-ben-code-flow`, not `Users-ben-code-flow`).
- **`session_id` in state file** — Start now captures `CLAUDE_SESSION_ID` from the
  environment and stores it in the state file for Complete to use.

## v0.24.8

### Fixes

- Fix pymarkdown MD018 violation in RELEASE-NOTES.md where issue references
  at the start of a continuation line were flagged as possible Atx headings.

## v0.24.7 — Fix Phase 4 Code Review halting bugs

### Fixes

- Phase 4 Code Review no longer halts between steps. Each step now
  self-invokes via `--continue-step` instead of relying on HARD-GATEs
  that the model ignores at Skill tool turn boundaries.
  Fixes #86, #87, #88.

### Improvements

- New `Mid-Phase Self-Invocation` rule in `.claude/rules/skill-authoring.md`
  documents the correct pattern for future skill authors.

## v0.24.6 — Auto-detect repo in bin/flow issue

### Improvements

- `bin/flow issue` now auto-detects the GitHub repo from `git remote origin` when `--repo` is omitted (#84, #85)
- Supports both SSH and HTTPS remote URL formats
- Falls back to a helpful JSON error if detection fails and no `--repo` is given

## v0.24.5 — Learn phase worktree fix and CI lint exclusion

### Fixes

- Learn skill now edits repo-destination files (CLAUDE.md, .claude/rules/) in the worktree instead of the project root, so changes land on the feature branch (#81, closes #80)
- Issues #69 and #70 closed as already fixed in v0.24.4
- Exclude tmp/ from pymarkdown linting — generated release notes artifacts no longer cause CI failures

## v0.24.4 — Fix Complete skill project root handling

### Fixes

- Fix Complete skill running from worktree instead of project root, which caused merge checkout failures (#79).

## v0.24.3 — Bug fixes for Code Review and Plan phases

### Fixes

- Fix Code Review step stalling when sub-agents encounter errors (#64, #65, #66, #67).
- Fix PR body rendering literal `\n` instead of newlines (#71).

### Improvements

- Port `bin/flow issue` command from maintainer to plugin for consistent permission handling (#73).

## v0.24.2 — Permission promotion and issue filing fixes

### Fixes

- Learn phase now promotes settings.local.json into settings.json in all modes (Phase 5, Maintainer, and Standalone), not just Maintainer — prevents accumulated session permissions from being left behind as uncommitted changes.

### Improvements

- Ported `bin/flow issue` command from maintainer-only to the plugin, so all projects can file GitHub issues through the permission-safe wrapper.
- Skills no longer write files to `/tmp/` — all file operations stay within the project root to avoid permission prompts.

## v0.24.1 — Bug fixes and resilience improvements

### Fixes

- Fix bin/flow commands failing when run from the main repo root instead of a worktree (#53).
- Fix Start pausing for confirmation after branch name truncation instead of proceeding automatically.
- Fix Learn phase hanging when an Edit tool call is denied — rejected learnings now skip gracefully (#54).
- Fix Code Review losing step progress after context compaction — completed steps are now tracked and skipped on resume (#50).
- Block pipe operators in PreToolUse hook to prevent sub-agent piped commands from bypassing permission matching (#55).

### Improvements

- Route GitHub issue creation through bin/flow issue for consistent permission matching.
- Enforce /flow:flow-commit routing in flow-prime and exclude bin/dependencies from bin/ci.

## v0.24.0 — Code Review plugin integration

### New features

- **Code Review Step 4** — Phase 4 now includes a 4th step that invokes
  the `code-review:code-review` plugin for multi-agent validation with
  CLAUDE.md compliance checking. Four parallel agents with a confidence
  threshold filter for high-signal findings only.
- **flow-prime installs code-review plugin** — New projects get the
  code-review plugin installed automatically during `/flow-prime`.

### Fixes

- Fix flow-prime blank line after FLOW:BEGIN marker
- Fix flow-prime Step 7 commit convention

### Improvements

- Phase skills now log completion events to `.flow-states/<branch>.log`
- Add project rule prohibiting direct Python invocation (use bin/flow wrappers)
- Configure FLOW workspace permissions

## v0.23.1 — Global Bash validator and CI sentinel fixes

- **Global PreToolUse hook** — `validate-ci-bash.py` is now registered
  globally in `hooks/hooks.json` for all Bash calls, not just ci-fixer.
  This prevents user-facing permission prompts when `/simplify` sub-agents
  use compound commands during Code Review.
- **CI sentinel fix** — The dirty-check sentinel now survives commits and
  is scoped per branch, fixing false "already green" skips after a commit.

## v0.23.0 — Rename Phase 6: Cleanup → Complete with PR merge automation

Phase 6 renamed from "Cleanup" to "Complete" everywhere: phase key
`flow-cleanup` → `flow-complete`, skill directory, command, and display
name. The phase now handles the full PR merge lifecycle instead of
requiring manual merge.

### Breaking changes

- Skill renamed: `flow-cleanup` → `flow-complete` (directory, command, state keys)
- Config key renamed: `skills.flow-cleanup` → `skills.flow-complete` in `.flow.json`
- Re-run `/flow:flow-prime` after upgrading to update permissions and config

### New behavior

- **PR merge automation** — Phase 6 now fetches main, resolves merge conflicts
  inline, checks CI status, and squash-merges the PR via `gh pr merge --squash
  --delete-branch`
- **Idempotent design** — safe to re-invoke via `/loop 15s /flow:flow-complete`
  for unattended merge-when-ready
- **CI failure handling** — launches ci-fixer sub-agent for failed checks,
  suggests `/loop` for pending checks
- **Conflict resolution** — Claude resolves merge conflicts inline using the
  Edit tool with full feature context, commits via `/flow:flow-commit`

### New permissions (4)

- `Bash(git fetch origin *)`, `Bash(git merge *)`, `Bash(gh pr checks *)`,
  `Bash(gh pr merge *)`

## v0.22.0 — Rename Phase 5: Learning → Learn

Rename Phase 5 from "Learning" to "Learn" to match the verb pattern
of the other phases (Start, Plan, Code, Code Review, Learn, Cleanup).

### Breaking changes

- Skill renamed: `flow-learning` → `flow-learn` (directory, command, state keys)
- Active features created before this version will have `flow-learning` keys in
  their state files. Complete or abort them before upgrading, or manually update
  the state file keys.

### Changes

- Display name "Learning" → "Learn" across all skills, docs, and config
- Phase key `flow-learning` → `flow-learn` in `flow-phases.json`
- Skill directory `skills/flow-learning/` → `skills/flow-learn/`
- Command `/flow:flow-learning` → `/flow:flow-learn`
- Doc files renamed: `phase-5-learning.md` → `phase-5-learn.md`,
  `flow-learning.md` → `flow-learn.md`
- Feature-level names preserved: "The Learning System", "Memory and Learning System"
- 28 files changed, pure mechanical rename with no logic changes

## v0.21.4 — Fix auto-upgrade display and Start phase halt on empty commit

### Fixes

- `lib/prime-check.py` now emits `old_version` and `new_version` in the
  auto-upgrade JSON output. Previously both fields were missing, causing
  the Start phase to display "v0.21.3 to v0.21.3" in the upgrade notice.
- `skills/flow-commit/SKILL.md`: nothing-to-commit path now prints the
  COMPLETE banner and returns to the caller instead of halting. Previously
  "stop" caused the entire Start phase to halt when there were no dependency
  changes to commit.
- `skills/flow-start/SKILL.md`: Step 1 upgrade notice references
  `old_version`/`new_version` fields by name. Step 6 adds a `git status`
  pre-check to skip `flow-commit` when there is nothing to stage.
- `tests/test_prime_check.py`: added assertions for `old_version` and
  `new_version` in `test_auto_upgrades_when_config_hash_matches`.

## v0.21.3 — CI fixer sub-agent, Code Review fix, docs improvements

### Fixes

- Replace general-purpose sub-agent with custom `ci-fixer` plugin sub-agent
  (`agents/ci-fixer.md`) to eliminate permission prompts during autonomous Start
  phase CI fixes. Uses a `PreToolUse` hook (`lib/validate-ci-bash.py`) to enforce
  tool restrictions at the system level instead of unreliable prompt-level rules.
  Closes #35 and #44.
- Fix Code Review stopping after a no-findings sub-skill returns a blank prompt
  instead of continuing to the next review lens. Closes #43.

### Improvements

- Restructure CLAUDE.md to give Claude a better mental model of FLOW from the
  first 30 lines — design philosophy and phase table moved up front.
- Make `bin/dependencies` quiet and self-updating so it runs clean every time
  without noisy output.

## v0.21.2 — Fix bin/dependencies venv detection and worktree file visibility

### Fixes

- Fix bin/dependencies to detect and use .venv/bin/pip instead of bare pip,
  matching the pattern already used in bin/ci and bin/test
- Fix flow-start Step 5 to use Read tool instead of Glob for checking
  bin/dependencies existence — Glob cannot see files inside worktrees
- Update frameworks/python/dependencies template with the same venv fix
- Remove accidentally tracked .flow.json from the repo

## v0.21.1 — Bug fixes for session-start hook and flow-start

### Fixes

- Session-start hook: filter out ghost `-phases.json` files that created phantom
  "None — Start" features in multi-feature contexts
- Session-start hook: add plan-aware logic to multi-feature branch so approved
  plans trigger auto-continue instead of "do not invoke"
- Session-start hook: add implementation guardrail to all branches preventing
  direct implementation after context compaction
- Flow-start: restore missing feature name prompt, fix CI failure dead-end, and
  strengthen commit enforcement

## v0.21.0 — Rename flow-init → flow-prime

- **Renamed /flow-init → /flow-prime** — Skill directory, lib scripts, test
  files, docs, and all cross-references. "Prime the project" reads better than
  "init" alongside the existing prime-project priming step.
- **Bug fix: autonomy config now works in worktrees** — `/flow-start` copies
  the `skills` config from `.flow.json` into the state file. Phase skills read
  autonomy settings from the state file instead of `.flow.json`, which isn't
  accessible from worktrees. Previously, Customize settings were silently
  ignored.
- **.flow.json excluded from git** — Added to `.git/info/exclude` during
  `/flow-prime`. Per-user autonomy preferences no longer need to be committed.
- **Recommended labels in Customize prompts** — Each per-skill autonomy prompt
  now shows "(Recommended)" on the suggested default.

## v0.20.1 — Fix plugin loading

### Fixes

- Remove `config_hash` from plugin.json — Claude Code's manifest
  validator rejects unrecognized keys, preventing the plugin from
  loading. The hash is now computed dynamically at runtime instead
  of stored.

## v0.20.0 — Init versioning and collapsed phases

### New

- **Init versioning** — `flow-init` now writes a version marker to the target
  project, enabling version-aware upgrade detection.
- **Collapsed phases** — Phase status display collapses completed phases for a
  cleaner overview.

### Improvements

- Banner fence pattern standardized to `` ````markdown `` + `` ```text `` across
  all skills for consistent rendering.
- Removed `flow:` namespace prefix from user-facing docs; maintainer skills
  renamed for clarity.
- `/flow-qa` rewritten to use plugin uninstall/install for reliable dev-mode
  toggling; bare invocation now shows status instead of starting.
- Commit subjects now require full sentences (imperative verb + period).

## v0.19.1 — Block compound shell commands and fix init instruction

### Fixes

- **Permission prompts** — Claude was appending `; echo EXIT:$?` to clean
  bash commands in skills, triggering permission prompts. Added `Bash(* ; *)`
  to the deny list alongside the existing `Bash(* && *)` pattern. Both are
  now propagated to target projects via `init-setup`.
- **flow-init Done instruction** — The Done section told users to run
  `/flow:flow-start` (full namespace prefix) instead of `/flow-start`.

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
