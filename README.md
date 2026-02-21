# FLOW — Rails Development Lifecycle for Claude Code

An opinionated 8-phase development plugin for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) that enforces research-first, design-first, TDD discipline on every feature in a Ruby on Rails codebase.

**Every feature. Same 8 phases. Same order. No shortcuts.**

**Documentation:** [benkruger.github.io/flow](https://benkruger.github.io/flow)

---

## The Problem

Claude Code is powerful, but undisciplined by default.

FLOW imposes structure. Not bureaucracy — discipline.

---

## The Workflow

```
Start → Research → Design → Plan → Code → Review → Reflect → Cleanup
  1         2         3       4      5        6        7          8
```

| Phase | Command | What happens |
|-------|---------|-------------|
| **1: Start** | `/flow:start <name>` | New worktree, push branch, open PR, upgrade gems, `bin/ci` green baseline |
| **2: Research** | `/flow:research` | Sub-agent reads full class hierarchy, finds callbacks, checks `test/support/`, documents risks |
| **3: Design** | `/flow:design` | Sub-agent validates 2-3 alternatives, user picks one, design is approved before any code |
| **4: Plan** | `/flow:plan` | Sub-agent verifies tasks are executable, section-by-section approval, TDD ordering |
| **5: Code** | `/flow:code` | Test-first per task, diff review before `bin/ci`, commit per task, 100% coverage enforced |
| **6: Review** | `/flow:review` | Sub-agent checks design alignment, research risk coverage, Rails anti-patterns |
| **7: Reflect** | `/flow:reflect` | Corrections become reusable patterns, CLAUDE.md updated, plugin gaps noted |
| **8: Cleanup** | `/flow:cleanup` | Worktree removed, state file deleted, feature done |

---

## Installation

In any Claude Code session:

```
/plugin marketplace add benkruger/flow
/plugin install flow@flow-marketplace
```

Then start a feature:

```
/flow:start app payment webhooks
```

This creates branch `app-payment-webhooks`, a worktree at `.worktrees/app-payment-webhooks`, opens a GitHub PR, upgrades gems, and runs `bin/ci` to establish a green baseline. You land in Phase 2: Research.

---

## Utility Commands

Available at any point in the workflow:

| Command | What it does |
|---------|-------------|
| `/flow:commit` | Full diff review, approved commit message, pull before push |
| `/flow:status` | Current phase, PR link, cumulative time per phase, next step |
| `/flow:resume` | Re-asks last transition question; rebuilds context on new session |
| `/flow:note` | Captures corrections to state file — auto-invoked when Claude is wrong |

---

## Architecture

### Sub-Agent Architecture

Phases that read the codebase (Research, Design, Plan, Review) launch mandatory Explore-type sub-agents. The main conversation stays focused on decisions. The sub-agent handles all file reads and returns structured findings.

```
Main conversation          Sub-agent (Explore)
      |                          |
      |─── Task: explore ───────>|
      |    (what to look for)    |─── Read models
      |                          |─── Find callbacks
      |                          |─── Check test/support/
      |                          |─── Scan routes, schema...
      |<── Structured findings ──|
      |
      |─── Makes decisions
      |─── Asks user questions
      |─── Updates state file
```

By the time Code begins, every affected file has been read, every callback has been found, every risk has been documented. Code doesn't re-explore — it trusts the state file. This keeps the main context clean for decision-making throughout a long session.

### State File Persistence

Every feature has a state file at `.claude/flow-states/<branch>.json`. It stores:

- **Research findings** — affected files, callbacks, risks, clarifications
- **Design decisions** — chosen approach, schema/model/controller/worker changes, rationale
- **Plan tasks** — ordered, section-by-section, with TDD flags and status
- **Notes** — corrections captured automatically throughout the session
- **Timing** — per-phase cumulative seconds and visit counts

State survives session breaks and compaction. Multiple features can run simultaneously in separate worktrees with separate state files.

### Session Hook — Auto-Resume

Every Claude Code session start — new terminal, `/clear`, `/compact` — triggers a hook that scans `.claude/flow-states/` for in-progress features.

If a feature is found, Claude's **first action** is to invoke `/flow:resume`. No prompt needed. No "what were we working on?" You close your laptop, open Claude Code the next morning, and the session opens with your feature's current phase, PR link, and time spent — then asks one question: "Ready to continue Phase 4: Plan?" Say yes and you're back exactly where you left off.

If two features are in progress across two worktrees, the hook asks which one to resume before proceeding.

The same hook also injects the correction-capture instruction for the full session:

> "Throughout this session: whenever the user corrects you, invoke `/flow:note` immediately before replying."

Both behaviors — auto-resume and correction capture — are wired in at session start, without any user action.

### The Learning Pipeline

Every correction Claude makes has a path to becoming a permanent, reusable pattern:

```
User corrects Claude
       ↓
/flow:note captures it as a reusable pattern in state["notes"]
       ↓
Reflect phase synthesizes all notes from the full feature
       ↓
Each approved pattern is added to CLAUDE.md
       ↓
Every future feature in this Rails project benefits
```

The learnings don't evaporate at session end. They compound.

### Phase Back-Navigation

Every phase that allows it offers back-navigation when something was missed:

| Phase | Can return to |
|-------|--------------|
| Research | Start |
| Design | Research |
| Plan | Design, Research |
| Code | Plan, Design, Research |
| Review | Code |

When returning, state is reset appropriately. Later phases are invalidated. Prior findings are preserved and extended — never discarded.

---

## What It Enforces

- **Worktree isolation** — main is never touched directly; multiple features run in parallel
- **Research before design** — full class hierarchy read, callbacks found, risks documented
- **Design alternatives required** — 2-3 distinct approaches validated before user picks one
- **TDD always** — test must fail before implementation is written; test must pass before commit
- **`bin/ci` gate** — must be green before every commit and every phase transition
- **100% test coverage** — Code phase cannot transition to Review without it
- **No disabling RuboCop** — fix the code, not the cop; no `# rubocop:disable` comments
- **Commit discipline** — imperative verb + tl;dr + per-file breakdown, every commit

---

## What Gets Built Per Feature

Every completed feature produces:

- A merged PR with clean, TDD-tested, reviewed code
- Individual commits per plan task with detailed messages
- 100% test coverage maintained
- All research risks addressed (verified by Review phase)
- New CLAUDE.md patterns from corrections and learnings
- A clean state file (deleted at Cleanup)

---

## Instructions Are Advisory. Gates Aren't.

Most agent workflows put enforcement in instructions: "always run bin/ci", "never skip Research". Instructions work until they don't. FLOW's phase enforcement is layered and deterministic. There is no instruction path from an incomplete phase to the next one running.

Three independent mechanisms enforce this:

- **Inline phase guard** — every phase skill opens with a Python gate that reads the state file and exits immediately with `BLOCKED` if the previous phase isn't complete. The skill doesn't run — there's nothing for Claude to interpret or override.

- **`check-phase.py`** — a standalone verification script callable from anywhere in the workflow. One source of truth for phase state, used by skills, hooks, and utility commands alike.

- **SessionStart hook** — fires on every session start (`startup`, `/clear`, `/compact`). Reads the state file and injects the current phase directly into Claude's context. After a week away, Claude opens knowing exactly where it is and cannot proceed as if it doesn't.

---

## Part of the Ecosystem

FLOW is part of a growing community of disciplined Claude Code plugins. Two projects worth knowing that inspired and motivated me:

- **[metaswarm](https://github.com/dsifry/metaswarm)** by Dave Sifry — a multi-agent orchestration framework with 18 specialized agents, cross-model adversarial review, and full pipeline orchestration from GitHub issue to merged PR. If FLOW is disciplined Rails development, metaswarm is disciplined development at scale.

- **[Superpowers](https://github.com/obra/superpowers)** by Jesse Vincent — foundational agentic skills for Claude Code including TDD, systematic debugging, and plan writing. Proved that disciplined agent workflows are not overhead — they're what make autonomous development reliable.

---

## Updating

```
/plugin update flow@flow-marketplace
```

---

## License

[MIT](LICENSE)
