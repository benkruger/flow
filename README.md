# FLOW — Software Development Lifecycle for Claude Code

An opinionated 6-phase development plugin for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) that enforces plan-first, TDD discipline on every feature. Supports Rails, Python, and iOS.

**Every feature. Same 6 phases. Same order. No shortcuts.**

**Documentation:** [benkruger.github.io/flow](https://benkruger.github.io/flow)

---

## Why FLOW

Claude Code is powerful, but undisciplined by default. FLOW imposes structure. Not bureaucracy — discipline. DAG decomposition for planning, then TDD execution, then four-lens code review, then learnings that compound. Every feature, same order.

---

## Three Goals

### Unobtrusive

Zero dependencies — pure Markdown skills with a thin Python dispatcher. Nothing is committed — `.flow.json` and `.claude/settings.json` are git-excluded (via `/flow-prime`, once per project). During active development, a single gitignored JSON state file exists at `.flow-states/<branch>.json`. When the feature completes, that file is deleted too. Three commands to set up. One file while you work. Zero when you're done.

### Autonomous or Manual

Every skill has two independent axes — **commit** (show diffs or auto-commit) and **continue** (prompt before advancing or auto-advance). Start fully manual. Dial up autonomy per skill as comfort grows. Go fully autonomous when you trust the workflow. See [Autonomy](#you-control-the-autonomy) below.

### Safe for Local Env

No containers. No external dependencies. Native tools only — git, gh, your linter, your test runner. Every command is pre-approved in `.claude/settings.json` so you never see a permission prompt. Worktree isolation protects main — multiple features run in parallel without touching your working branch.

### Slack Notifications

Optional thread-per-feature notifications give your team passive awareness of feature progress. Each feature gets one Slack thread — every phase posts a reply, building a narrative from start to merge. Set two env vars, run `/flow-prime`, done. See [Slack Integration](docs/integrations/slack.md).

---

## The Workflow

```text
Start → Plan → Code → Code Review → Learn → Complete
  1       2      3         4            5          6
```

| Phase | Command | What happens |
|-------|---------|-------------|
| **1: Start** | `/flow-start <prompt>` | Lock, pull main, `bin/ci` baseline, upgrade dependencies, `bin/ci` post-deps, commit to main, unlock, new worktree + PR — ci-fixer sub-agent handles failures |
| **2: Plan** | `/flow-plan` | Reads the start prompt, invokes DAG decompose plugin for dependency analysis, explores codebase, produces ordered tasks with dependency graph |
| **3: Code** | `/flow-code` | Test-first per task, diff review before `bin/ci`, commit per task, 100% coverage enforced |
| **4: Code Review** | `/flow-code-review` | Three built-in lenses — clarity (inline review passes), correctness (inline review, rule compliance), safety (inline security review) — plus optional CLAUDE.md compliance (`code-review:code-review` plugin, configurable: `always`/`auto`/`never`) |
| **5: Learn** | `/flow-learn` | Learnings routed to CLAUDE.md, rules, and memory — plugin gaps noted |
| **6: Complete** | `/flow-complete` | Close issues referenced in prompt, PR merged, worktree removed, state file deleted, feature done |

---

## Guardrails

- **`bin/ci` is the universal gate** — must be green before every commit and every phase transition. Recommend keeping guardrails under 2 minutes for tight feedback loops.
- **100% test coverage required** — Code phase cannot advance to Code Review without it.
- **TDD always** — test must fail before implementation is written; test must pass before commit.
- **No lint suppression** — fix the code, not the linter. No exclusions, no suppression comments.
- **Worktree isolation** — main is never touched directly; multiple features run in parallel.
- **Commit discipline** — imperative verb + tl;dr + per-file breakdown, every commit.

---

## You Control the Autonomy

Every skill has two independent axes you can tune:

- **Commit** — whether Claude shows diffs for approval or commits autonomously
- **Continue** — whether Claude prompts before advancing to the next phase or auto-advances

Start fully manual. As your comfort grows, dial up autonomy per skill. Go fully autonomous when you trust the workflow.

### Four preset levels via `/flow-prime`

| Level | What it means |
|-------|--------------|
| **Fully autonomous** | All skills auto for both axes — zero prompts |
| **Fully manual** | Every diff reviewed, every phase transition confirmed |
| **Recommended** | Auto where safe (Code Review), manual where judgment matters (Code, Plan) |
| **Customize** | Choose per skill and per axis |

### Runtime overrides

Any skill invocation accepts `--auto` or `--manual` to override the configured setting for that run:

```text
/flow-code --auto        # skip per-task approval for this session
/flow-code-review --manual  # prompt before advancing, just this once
```

### Configuration lives in `.flow.json`

```json
{
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-code-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto"
  }
}
```

View your current settings anytime with `/flow-config`.

---

## Installation

In any Claude Code session:

```bash
/plugin marketplace add benkruger/flow
/plugin install flow@flow-marketplace
```

Then initialize in your project (once per project, and again after each FLOW upgrade):

```bash
/flow-prime
```

Start a new Claude Code session so permissions take effect, then start a feature:

```bash
/flow-start invoice pdf export
```

This acquires a start lock (serializing concurrent starts), pulls main, runs `bin/ci` for a clean baseline, upgrades dependencies on main, runs `bin/ci` again to catch dep-induced breakage, commits everything to main, then creates branch `invoice-pdf-export` with a worktree at `.worktrees/invoice-pdf-export` and opens a GitHub PR. You land in Phase 2: Plan.

---

## Utility Commands

Available at any point in the workflow:

| Command | What it does |
|---------|-------------|
| `/flow-prime` | One-time project setup — configure permissions and git excludes |
| `/flow-commit` | Full diff review, approved commit message, pull before push |
| `/flow-status` | Current phase, PR link, cumulative time per phase, next step |
| `/flow-continue` | Re-asks last transition question; rebuilds full context on new session |
| `/flow-note` | Captures corrections to state file — auto-invoked when Claude is wrong |
| `/flow-abort` | Abandon feature — close PR, delete remote branch, remove worktree, delete state |
| `/flow-reset` | Remove all FLOW artifacts — close PRs, delete worktrees/branches/state files |
| `/flow-config` | Display current configuration — version, framework, per-skill autonomy |
| `/flow-issues` | Fetch open issues, categorize, prioritize, and display a dashboard. Supports readiness filters |
| `/flow-create-issue` | Explore a design question or decompose a concrete problem, iterate until work-ready, then file it |
| `/flow-decompose-project` | Decompose a large project into linked GitHub issues with sub-issue relationships, blocked-by dependencies, and milestones |
| `/flow-orchestrate` | Process decomposed issues overnight — batch orchestration via flow-start --auto |
| `/flow-local-permission` | Promote permissions from settings.local.json into settings.json |

### Standalone Tools

| Command | What it does |
|---------|-------------|
| `flow tui` | Interactive terminal UI — view active flows, open worktrees, tail logs, abort features (no Claude session needed) |

### Terminal Dashboard

Monitor every active flow from your terminal — no Claude session needed. `flow tui` reads state files directly and auto-refreshes every 2 seconds, so phase transitions and code task progress appear as they happen.

| Key | Action |
|-----|--------|
| Up/Down | Navigate flow list |
| Left/Right | Switch tab |
| Enter | Open worktree in new Terminal tab |
| p | Open PR in browser |
| i | Open issue in browser |
| l | Show session log |
| a | Abort flow (with Y/N confirmation) |
| r | Force refresh |
| Esc | Back to list view |
| q | Quit |

The detail panel shows the full phase timeline with per-phase cumulative time, code task progress, diff stats, notes count, and issues filed. Runs standalone on macOS and Linux using the Python standard library `curses` module.

### Project Decomposition

Describe a project in plain language and FLOW decomposes it into a fully linked GitHub issue graph — epic, milestones, sub-issues, blocked-by dependencies, and phase labels. Every issue is filed work-ready with acceptance criteria, file paths, and scope boundaries from real codebase exploration.

```text
/flow-decompose-project add multi-tenant billing
```

The skill walks through 6 steps: DAG decomposition with codebase exploration, issue list review with iteration, epic and milestone creation, child issue filing in topological order, sub-issue and blocked-by relationship linking, and a final report. You review and iterate at each gate before anything is filed. The resulting issue graph feeds directly into `/flow-orchestrate` for overnight processing, or you pick issues one at a time with `/flow-start work on issue #N`.

### Batch Orchestration

Feed the issue graph into `/flow-orchestrate` and let FLOW process them overnight. It fetches open issues labeled "Decomposed", filters out any marked "Flow In-Progress", and runs each sequentially through all 6 phases via `flow-start --auto`.

The next time you open a Claude Code session, the session-start hook delivers a morning report: which issues completed (with PR links), which failed (with reasons), and total elapsed time. One command to start, zero intervention overnight, full accountability in the morning.

---

## Architecture

### Sub-Agent Architecture

Start uses a Sonnet sub-agent for CI failures. Plan invokes the `decompose` plugin (`decompose:decompose`) for DAG-based task decomposition. Code Review performs three inline review passes for clarity, then performs inline correctness review for correctness (including rule compliance), and inline security review for safety, and optionally the `code-review:code-review` plugin for multi-agent validation. Code has no sub-agent.

```text
Main conversation          Sub-agent (general-purpose)
      |                          |
      |─── Task: analyze ───────>|
      |    (what to check)       |─── Read affected code
      |                          |─── Find conventions/risks
      |                          |─── Check test infrastructure
      |                          |─── Scan dependencies...
      |<── Structured findings ──|
      |
      |─── Makes decisions
      |─── Asks user questions
      |─── Updates state file
```

Phase 1 uses the **ci-fixer sub-agent** when `bin/ci` fails — at the baseline CI gate and again after dependency upgrades. The sub-agent diagnoses failures, fixes them, iterates up to 3 times, then reports back. A file lock serializes concurrent starts so they do not fight over main.

### State File Persistence

Every feature has a state file at `.flow-states/<branch>.json`. Key fields include:

- **Identity** — `branch`, `repo`, `pr_number`, `pr_url`, `prompt`, `framework`
- **Phase tracking** — `current_phase`, per-phase `status`/`started_at`/`completed_at`/`cumulative_seconds`/`visit_count`, `phase_transitions` history
- **Artifact paths** — `files.plan`, `files.dag`, `files.log`, `files.state`
- **Progress** — `code_task` counter, `code_review_step`, `learn_step`, `complete_step`
- **Notes** — corrections captured via `/flow-note` throughout the session
- **Continuation** — `_continue_pending`, `_continue_context`, `_auto_continue` for stop-hook resumption
- **Compaction** — `compact_summary`, `compact_cwd`, `compact_count` for post-compaction context recovery
- **Autonomy** — `skills` object with per-skill `commit`/`continue` settings
- **Slack** — `slack_thread_ts`, `slack_notifications` for thread-per-feature tracking
- **Issues** — `issues_filed` array (Tech Debt, Flaky Test, Documentation Drift, Flow issues)
- **Diff stats** — `files_changed`, `insertions`, `deletions` captured at Code phase completion

Full schema reference: `docs/reference/flow-state-schema.md`.

State survives session breaks and compaction. Multiple features can run simultaneously in separate worktrees with separate state files — both on the same machine and across multiple engineers. State files are local to each machine; GitHub labels ("Flow In-Progress") provide cross-engineer WIP detection so `/flow-issues` shows which issues are already being worked on.

### Session Hook — Feature Awareness

Every Claude Code session start — new terminal, `/clear`, `/compact` — triggers a hook that scans `.flow-states/` for in-progress features.

If a feature is found, Claude knows the feature name, current phase, worktree, and code task progress — but does not act on it. No auto-prompting, no "Ready to continue?" When you want to resume, type `/flow-continue` and pick up exactly where you left off.

The hook also handles:

- **Timing recovery** — resets interrupted session timing so cumulative phase durations stay accurate across session breaks
- **Compaction recovery** — consumes `compact_summary` and `compact_cwd` from the state file to inject richer context after `/compact`
- **Orchestration awareness** — detects in-progress or completed orchestration runs and delivers the morning report
- **Correction capture** — injects the instruction to invoke `/flow-note` whenever the user corrects Claude

All behaviors are wired at session start without any user action.

### The Learning System

Every correction and observation has a path to becoming a permanent, reusable pattern — routed to the right home:

```text
User corrects Claude → /flow-note captures it in state["notes"]
Claude writes observations → auto-memory (shared across worktrees)
       ↓
Learn reads three sources (CLAUDE.md rules, conversation context, state/plan data)
       ↓
Each learning is routed to the right repo-local destination:
    → Project CLAUDE.md   (process rules and architecture — committed via PR)
    → Project rules       (coding anti-patterns and gotchas — committed via PR)
```

The learnings don't evaporate at session end. They compound.

### Bash Validation Hook

A global `PreToolUse` hook (`validate-ci-bash.py`) fires on every Bash call in any FLOW-primed project. It enforces 6 validation layers in order:

1. **Compound commands** — blocks `&&`, `;`, `|` (use separate Bash calls)
2. **Shell redirection** — blocks `>`, `>>`, `2>` (use Read/Write tools)
3. **Blanket restore** — blocks `git restore .` (restore files individually)
4. **Deny list** — blocks commands matching deny patterns in `.claude/settings.json`
5. **File-read commands** — blocks `cat`, `head`, `tail`, `grep`, `rg`, `find`, `ls` (use dedicated tools)
6. **Whitelist** — command must match a `Bash(...)` allow pattern in `.claude/settings.json`

Layers 1–5 are always enforced. Layer 6 (whitelist) is **flow-aware**: it only enforces during an active flow (when `.flow-states/<branch>.json` exists). Outside of flows, unlisted commands fall through to Claude Code's native permission system so users can still run `npm test`, `docker compose up`, or any other command by approving the prompt.

### Phase Back-Navigation

Phases that allow it offer back-navigation when something was missed:

| Phase | Can return to |
|-------|--------------|
| Code | Plan |
| Code Review | Code, Plan |

When returning, state is reset appropriately. Later phases are invalidated. Prior findings are preserved and extended — never discarded.

---

## What Gets Built Per Feature

Every completed feature produces:

- A merged PR with clean, TDD-tested, reviewed code
- Individual commits per plan task with detailed messages
- 100% test coverage maintained
- All identified risks addressed (verified by Review phase)
- New CLAUDE.md patterns from corrections and learnings
- A clean state file (deleted at Complete)

---

## Instructions Are Advisory. Gates Aren't

Most agent workflows put enforcement in instructions: "always run bin/ci", "never skip Plan". Instructions work until they don't. FLOW's phase enforcement is layered and deterministic. There is no instruction path from an incomplete phase to the next one running.

Three independent mechanisms enforce this:

- **Inline phase guard** — every phase skill opens with a Python gate that reads the state file and exits immediately with `BLOCKED` if the previous phase isn't complete. The skill doesn't run — there's nothing for Claude to interpret or override.

- **`check-phase.py`** — a standalone verification script callable from anywhere in the workflow. One source of truth for phase state, used by skills, hooks, and utility commands alike.

- **SessionStart hook** — fires on every session start (`startup`, `/clear`, `/compact`). Reads the state file and injects the current phase directly into Claude's context. After a week away, Claude opens knowing exactly where it is and cannot proceed as if it doesn't.

- **PostCompact hook** — fires after context compaction. Captures the conversation summary and CWD into the state file so the SessionStart hook can inject richer context on resume. Tracks compaction count per feature.

---

## Maintainer Tools

These skills and scripts live in the FLOW repo itself (`.claude/skills/` and `lib/`). They are not part of the user-facing plugin — they exist to develop, test, and release FLOW.

| Command | What it does |
|---------|-------------|
| `/flow-release` | Bump version in plugin.json and marketplace.json, tag, push, create GitHub Release |
| `/flow-qa` | End-to-end QA — clone QA repos, prime with local source, run a full 6-phase lifecycle, verify results |
| `/flow-changelog-audit` | Audit Claude Code CHANGELOG.md for plugin-relevant changes, categorize as Adopt/Remove/Adapt, file issues |

### QA System

Every plugin change can be tested end-to-end before releasing. `/flow-qa` clones dedicated QA repos, primes them with the local plugin source, runs a fully autonomous 6-phase lifecycle (Start through Complete), and verifies the results.

```bash
/flow-qa python       # test against Python QA repo
/flow-qa rails        # test against Rails QA repo
/flow-qa ios          # test against iOS QA repo
/flow-qa all          # test all frameworks sequentially
```

Each framework has a dedicated QA repo (`benkruger/flow-qa-python`, `flow-qa-rails`, `flow-qa-ios`) with a minimal Calculator class, tests, `bin/ci`, and seed issues. The QA skill clones fresh, primes with `--plugin-root $PWD` to test the local source, runs a flow against a seed issue, and then verifies: worktree removed, state file deleted, PR merged, no stale artifacts.

Supporting scripts: `bin/flow scaffold-qa` (create QA repos from templates), `bin/flow qa-reset` (reset repos to seed state), `bin/flow qa-verify` (verify post-Complete assertions).

For quick local testing without the full QA lifecycle, use Claude Code's `--plugin-dir` flag:

```bash
claude --plugin-dir=$HOME/code/flow
```

---

## Updating

From the command line:

```bash
claude plugin marketplace update flow-marketplace
```

---

## License

[MIT](LICENSE)
