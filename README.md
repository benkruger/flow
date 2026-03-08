# FLOW — Software Development Lifecycle for Claude Code

An opinionated 8-phase development plugin for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) that enforces plan-first, TDD discipline on every feature. Supports Rails and Python.

**Every feature. Same 8 phases. Same order. No shortcuts.**

**Documentation:** [benkruger.github.io/flow](https://benkruger.github.io/flow)

---

## The Problem

Claude Code is powerful, but undisciplined by default.

FLOW imposes structure. Not bureaucracy — discipline.

---

## Why FLOW

- **Plan mode** exploration, then TDD execution — every feature, same order
- **Zero dependencies** — pure Markdown skills with a thin Python dispatcher
- **Learning system** that routes corrections to CLAUDE.md, rules, and memory
- **Autonomy** on your terms — fully manual to fully autonomous, per skill
- **Opus** for planning and security, Sonnet for review and simplify, Haiku for setup
- **Rails** and Python today, more frameworks ahead
- **Minimal footprint** — `.flow-states` is the only artifact while you work, and Cleanup deletes even that

---

## The Workflow

```text
Start → Plan → Code → Simplify → Review → Security → Learning → Cleanup
  1       2      3       4          5          6          7         8
```

| Phase | Command | Model | What happens |
|-------|---------|-------|-------------|
| **1: Start** | `/flow:flow-start <name>` | Haiku | New worktree, push branch, open PR, `bin/ci` baseline, upgrade dependencies, `bin/ci` green — Sonnet sub-agent fixes CI failures |
| **2: Plan** | `/flow:flow-plan` | **Opus** | Native plan mode — explore codebase, design approach, produce ordered tasks with risks |
| **3: Code** | `/flow:flow-code` | **Opus** | Test-first per task, diff review before `bin/ci`, commit per task, 100% coverage enforced |
| **4: Simplify** | `/flow:flow-simplify` | Sonnet | Invoke `/simplify` on committed code, refactor for clarity, auto-commit |
| **5: Review** | `/flow:flow-review` | Sonnet | Sub-agent checks plan alignment, risk coverage, framework anti-patterns |
| **6: Security** | `/flow:flow-security` | **Opus** | Sub-agent scans diff for vulnerabilities, auth gaps, data exposure, injection risks |
| **7: Learning** | `/flow:flow-learning` | Sonnet | Learnings routed to CLAUDE.md, rules, and memory — plugin gaps noted |
| **8: Cleanup** | `/flow:flow-cleanup` | Haiku | Worktree removed, state file deleted, feature done |

---

## You Control the Autonomy

Every skill has two independent axes you can tune:

- **Commit** — whether Claude shows diffs for approval or commits autonomously
- **Continue** — whether Claude prompts before advancing to the next phase or auto-advances

Start fully manual. As your comfort grows, dial up autonomy per skill. Go fully autonomous when you trust the workflow.

### Four preset levels via `/flow:flow-init`

| Level | What it means |
|-------|--------------|
| **Fully autonomous** | All skills auto for both axes — zero prompts |
| **Fully manual** | Every diff reviewed, every phase transition confirmed |
| **Recommended** | Auto where safe (Simplify, Security), manual where judgment matters (Code, Plan) |
| **Customize** | Choose per skill and per axis |

### Runtime overrides

Any skill invocation accepts `--auto` or `--manual` to override the configured setting for that run:

```text
/flow:flow-code --auto        # skip per-task approval for this session
/flow:flow-security --manual  # prompt before advancing, just this once
```

### Configuration lives in `.flow.json`

```json
{
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-simplify": {"commit": "auto", "continue": "auto"},
    "flow-review": {"commit": "auto", "continue": "auto"},
    "flow-security": {"commit": "auto", "continue": "auto"},
    "flow-learning": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-cleanup": "auto"
  }
}
```

View your current settings anytime with `/flow:flow-config`.

---

## Installation

In any Claude Code session:

```bash
/plugin marketplace add benkruger/flow
/plugin install flow@flow-marketplace
```

Then initialize in your project (once per project, and again after each FLOW upgrade):

```bash
/flow:flow-init
```

Start a new Claude Code session so permissions take effect, then start a feature:

```bash
/flow:flow-start invoice pdf export
```

This creates branch `invoice-pdf-export`, a worktree at `.worktrees/invoice-pdf-export`, opens a GitHub PR, runs `bin/ci` to establish a baseline, upgrades dependencies, runs `bin/ci` again to confirm green, and lands you in Phase 2: Plan.

---

## Minimal Footprint

The plugin itself installs into Claude Code's managed plugin directory — one place, fully managed by Claude Code.

FLOW configures workspace permissions in `.claude/settings.json` and a version marker in `.flow.json` (via `/flow:flow-init`, committed once). During active development, a single gitignored JSON state file per feature exists at `.flow-states/<branch>.json`. When the feature is done and Cleanup runs, that file is deleted too.

**Three commands to set up. One file while you work. Zero when you're done.**

---

## Utility Commands

Available at any point in the workflow:

| Command | What it does |
|---------|-------------|
| `/flow:flow-init` | One-time project setup — configure permissions and git excludes |
| `/flow:flow-commit` | Full diff review, approved commit message, pull before push |
| `/flow:flow-status` | Current phase, PR link, cumulative time per phase, next step |
| `/flow:flow-continue` | Re-asks last transition question; rebuilds full context on new session |
| `/flow:flow-note` | Captures corrections to state file — auto-invoked when Claude is wrong |
| `/flow:flow-abort` | Abandon feature — close PR, delete remote branch, remove worktree, delete state |
| `/flow:flow-config` | Display current configuration — version, framework, per-skill autonomy |
| `/flow:flow-local-permission` | Promote permissions from settings.local.json into settings.json |

---

## Architecture

### Sub-Agent Architecture

Three phase skills launch mandatory sub-agents: Review and Security (general-purpose). Start uses a Sonnet sub-agent for CI failures. Plan uses Claude Code's native plan mode (`EnterPlanMode`/`ExitPlanMode`) instead of sub-agents. Code has no sub-agent.

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

Phase 1 also uses a **general-purpose Sonnet sub-agent** when `bin/ci` fails — whether from a dirty main branch, dependency upgrade breakage, or flaky tests. The sub-agent diagnoses failures, fixes them, iterates up to 3 times, then reports back. The main Haiku agent handles the mechanical setup at speed.

### Model Recommendations

FLOW automatically selects the right model for each phase — Opus for hard thinking, Sonnet for structured work, Haiku for mechanical steps. Each skill's frontmatter sets the model, so invoking the skill switches automatically.

| Phase | Model | Why |
|-------|-------|-----|
| 1: Start | Haiku | Mechanical setup; CI failures delegated to Sonnet sub-agent |
| 2: Plan | **Opus** | Codebase exploration, architectural judgment, and task planning — bad plans cascade through all later phases |
| 3: Code | **Opus** | Writing correct code against complex codebase |
| 4: Simplify | Sonnet | Invoke `/simplify` for clarity refactoring, auto-commit |
| 5: Review | Sonnet | Sub-agent analyzes diff, fixes are targeted and small |
| 6: Security | **Opus** | Security analysis requires architectural reasoning about attack vectors and data flows |
| 7: Learning | Sonnet | Synthesizing learnings into reusable patterns |
| 8: Cleanup | Haiku | Delete worktree and state file |
| Commit | Sonnet | Writing clear, well-structured commit messages |

### State File Persistence

Every feature has a state file at `.flow-states/<branch>.json`. It stores:

- **Plan file path** — reference to the plan file at `~/.claude/plans/<name>.md`
- **Notes** — corrections captured automatically throughout the session
- **Timing** — per-phase cumulative seconds and visit counts
- **Security findings** — vulnerability tracking with fix status

State survives session breaks and compaction. Multiple features can run simultaneously in separate worktrees with separate state files.

### Session Hook — Feature Awareness

Every Claude Code session start — new terminal, `/clear`, `/compact` — triggers a hook that scans `.flow-states/` for in-progress features.

If a feature is found, Claude knows the feature name, current phase, and worktree — but does not act on it. No auto-prompting, no "Ready to continue?" interrupting your train of thought. When you want to resume, type `/flow:flow-continue` and pick up exactly where you left off.

The same hook injects the correction-capture instruction for the full session:

> "Throughout this session: whenever the user corrects you, invoke `/flow:flow-note` immediately before replying."

Both behaviors — feature awareness and correction capture — are wired in at session start, without any user action.

### The Learning System

Every correction and observation has a path to becoming a permanent, reusable pattern — routed to the right home:

```text
User corrects Claude → /flow:flow-note captures it in state["notes"]
Claude writes observations → auto-memory (shared across worktrees)
       ↓
Learning reads three sources (CLAUDE.md rules, conversation context, state/plan data)
       ↓
Each learning is routed to the right destination:
  Instructions (rules Claude must follow):
    → Global CLAUDE.md    (process rules for all projects)
    → Project CLAUDE.md   (project architecture — committed via PR)
    → Global rules        (universal coding standards)
    → Project rules       (project-specific gotchas — committed via PR)
  Context (knowledge Claude should know):
    → Project memory      (patterns, preferences, working knowledge)
```

The learnings don't evaporate at session end. They compound.

### Phase Back-Navigation

Phases that allow it offer back-navigation when something was missed:

| Phase | Can return to |
|-------|--------------|
| Code | Plan |
| Simplify | Code |
| Review | Code, Plan |

When returning, state is reset appropriately. Later phases are invalidated. Prior findings are preserved and extended — never discarded.

---

## What It Enforces

- **Worktree isolation** — main is never touched directly; multiple features run in parallel
- **Plan before code** — codebase explored, risks identified, approach approved before any implementation
- **TDD always** — test must fail before implementation is written; test must pass before commit
- **`bin/ci` gate** — must be green before every commit and every phase transition
- **100% test coverage** — Code phase cannot transition to Simplify without it
- **No disabling linters** — fix the code, not the linter; no lint suppression comments
- **Commit discipline** — imperative verb + tl;dr + per-file breakdown, every commit

---

## What Gets Built Per Feature

Every completed feature produces:

- A merged PR with clean, TDD-tested, reviewed code
- Individual commits per plan task with detailed messages
- 100% test coverage maintained
- All identified risks addressed (verified by Review phase)
- New CLAUDE.md patterns from corrections and learnings
- A clean state file (deleted at Cleanup)

---

## Instructions Are Advisory. Gates Aren't

Most agent workflows put enforcement in instructions: "always run bin/ci", "never skip Plan". Instructions work until they don't. FLOW's phase enforcement is layered and deterministic. There is no instruction path from an incomplete phase to the next one running.

Three independent mechanisms enforce this:

- **Inline phase guard** — every phase skill opens with a Python gate that reads the state file and exits immediately with `BLOCKED` if the previous phase isn't complete. The skill doesn't run — there's nothing for Claude to interpret or override.

- **`check-phase.py`** — a standalone verification script callable from anywhere in the workflow. One source of truth for phase state, used by skills, hooks, and utility commands alike.

- **SessionStart hook** — fires on every session start (`startup`, `/clear`, `/compact`). Reads the state file and injects the current phase directly into Claude's context. After a week away, Claude opens knowing exactly where it is and cannot proceed as if it doesn't.

---

## Part of the Ecosystem

FLOW is part of a growing community of disciplined Claude Code plugins. Two projects worth knowing that inspired and motivated me:

- **[metaswarm](https://github.com/dsifry/metaswarm)** by Dave Sifry — a multi-agent orchestration framework with 18 specialized agents, cross-model adversarial review, and full pipeline orchestration from GitHub issue to merged PR. If FLOW is disciplined feature development, metaswarm is disciplined development at scale.

- **[Superpowers](https://github.com/obra/superpowers)** by Jesse Vincent — foundational agentic skills for Claude Code including TDD, systematic debugging, and plan writing. Proved that disciplined agent workflows are not overhead — they're what make autonomous development reliable.

---

## Maintainer Tools

These skills and scripts live in the FLOW repo itself (`.claude/skills/` and `lib/`). They are not part of the user-facing plugin — they exist to develop, test, and release FLOW.

| Command | What it does |
|---------|-------------|
| `/release` | Bump version in plugin.json and marketplace.json, tag, push, create GitHub Release |
| `/flow-qa` | `--start`/`--stop` dev mode — nukes plugin cache, swaps marketplace source, tracks via `.dev-mode` marker |
| `/reset` | Remove all FLOW artifacts — close PRs, delete worktrees/branches/state files |

### Local QA Workflow

Every plugin change can be tested locally before releasing:

```bash
/flow-qa --start
```

This nukes the plugin cache directory, re-registers the marketplace to point at the local source, and updates the cache. Open a new Claude Code session in a target project to test. When done:

```bash
/flow-qa --stop
```

This nukes the cache again, restores the marketplace to the GitHub source, and updates. A `.flow-states/.dev-mode` marker tracks whether dev mode is active.

The underlying commands can also be run directly:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
claude plugin marketplace add /path/to/flow    # point cache at local source
claude plugin marketplace update flow-marketplace
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
