# FLOW — Software Development Lifecycle for Claude Code

An opinionated 5-phase development plugin for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) that enforces issue-driven, TDD discipline on every feature. Language-agnostic — every project owns its toolchain via `bin/format`, `bin/lint`, `bin/build`, `bin/test` stubs that FLOW orchestrates.

**Every feature. Same 5 phases. Same order. No shortcuts.**

**Documentation:** [benkruger.github.io/flow](https://benkruger.github.io/flow)

---

## Why FLOW

Claude Code is powerful, but undisciplined by default. FLOW imposes structure. Not bureaucracy — discipline. DAG decomposition for planning, then TDD execution, then four-agent code review, then learnings that compound. Every feature, same order.

---

## Four Tenets

### Unobtrusive

Zero dependencies — pure Markdown skills with a Rust dispatcher. Prime commits `.claude/settings.json` and the four `bin/*` delegation stubs (`bin/format`, `bin/lint`, `bin/build`, `bin/test`) as project config. Each project owns its own toolchain inside those scripts; FLOW provides only the orchestration layer. `.flow.json` and `.flow-states/` are git-excluded. During active development, a single gitignored JSON state file exists at `.flow-states/<branch>/state.json`. When the feature completes, that file is deleted too. Three commands to set up. One file while you work. Zero when you're done.

### Autonomous or Manual

Every skill has two independent axes — **commit** (show diffs or auto-commit) and **continue** (prompt before advancing or auto-advance). Start fully manual. Dial up autonomy per skill as comfort grows. Go fully autonomous when you trust the workflow. See [Autonomy](#you-control-the-autonomy) below.

### Safe for Local Env

No containers. No external dependencies. Native tools only — git, gh, your linter, your test runner. Every command is pre-approved in `.claude/settings.json` so you never see a permission prompt. A global `PreToolUse` hook blocks compound commands, shell redirection, and other footguns so the model can't reach around the gate. Worktree isolation protects your team's trunk (`main`, `staging`, or whatever your repo's default branch is) — multiple features run in parallel without touching it.

### N × N × N Concurrent

N engineers running N flows on N machines simultaneously is the primary use case. Local state (`.flow-states/`, worktrees) is per-machine; shared state (PRs, issues, labels) is coordinated through GitHub. The "Flow In-Progress" label provides cross-engineer WIP detection so `/flow-issues` shows which issues are already being worked on. Nothing assumes a single active flow.

---

## The Workflow

You type three commands. FLOW handles the rest.

### The three commands you type

| Step | Command | What you get |
|------|---------|--------------|
| 1 | `/flow-explore <topic>` | A vanilla `## What` / `## Why` / `## Acceptance Criteria` issue filed on GitHub (PM voice) |
| 2 | `/flow-plan #<issue>` or `/flow-plan <topic>` | An implementation plan attached to a GitHub issue. `#N` re-plans the existing issue in place; a bare topic synthesizes `## What` / `## Why` / `## Acceptance Criteria` and files a new decomposed issue (Tech Lead voice; mandatory `decompose:decompose` pass) |
| 3 | `/flow-start #<issue>` | Worktree, PR, plan extraction from the issue body — and the lifecycle begins |

```text
/flow-explore add a per-flow budget cap
/flow-plan #1234
/flow-start #1234
```

### The five phases that run after `/flow-start`

Once `/flow-start` lands, you're inside the lifecycle. Each phase is its own skill, but **you don't type them** — Claude auto-chains Code → Review → Learn → Complete based on your `.flow.json` autonomy settings. You see them as phase transitions, and as approval prompts at any boundary you've kept `continue: manual`.

```text
1: Start  →  2: Code  →  3: Review  →  4: Learn  →  5: Complete
```

| Phase | Command | What happens |
|-------|---------|-------------|
| **1: Start** | `/flow-start` | Acquire start lock, run `bin/flow ci` baseline on the integration branch, upgrade dependencies, commit, unlock, then create worktree + PR. `ci-fixer` sub-agent repairs any dependency breakage once; subsequent flows inherit the fix via the CI sentinel. Plan is extracted from the issue body's `<!-- FLOW-PLAN-BEGIN -->`/`<!-- FLOW-PLAN-END -->` sentinels. |
| **2: Code** | `/flow-code` | Test-first per task, diff review before `bin/flow ci`, commit per task, 100% coverage enforced. |
| **3: Review** | `/flow-review` | Four cognitively isolated agents in parallel — `reviewer`, `pre-mortem`, `adversarial`, `documentation`. Parent triages findings and fixes in-scope issues. |
| **4: Learn** | `/flow-learn` | Learnings routed to project `CLAUDE.md` and `.claude/rules/`; plugin process gaps filed as GitHub issues. |
| **5: Complete** | `/flow-complete` | Merge the PR, close issues referenced in the prompt, remove the worktree, delete the state file. |

Maintainer-only commands (private to this repo): `/flow-qa` files a pre-decomposed QA issue against the FLOW plugin repo for end-to-end lifecycle regression testing; `/flow-release` ships a tagged version; `/flow-changelog-audit` reviews Claude Code's CHANGELOG for plugin-relevant changes.

---

## Guardrails

- **`bin/flow ci` is the universal gate** — must be green before every commit and every phase transition. Recommend keeping guardrails under 2 minutes for tight feedback loops.
- **100% test coverage required** — Code phase cannot advance to Review without it.
- **TDD always** — test must fail before implementation is written; test must pass before commit.
- **No lint suppression** — fix the code, not the linter. No exclusions, no suppression comments.
- **Worktree isolation** — your team's trunk is never touched directly; multiple features run in parallel.
- **Commit discipline** — imperative verb + tl;dr + per-file breakdown, every commit.

---

## You Control the Autonomy

Every skill has two independent axes you can tune:

- **Commit** — whether Claude shows diffs for approval or commits autonomously
- **Continue** — whether Claude prompts before advancing to the next phase or auto-advances

Start fully manual. As your comfort grows, dial up autonomy per skill. Go fully autonomous when you trust the workflow.

### Configuration via `/flow-prime`

`/flow-prime` is the configuration front door. Type it yourself in any Claude Code session — the plugin walks you through a preset picker, writes the result to `.flow.json` at your project root, and installs the required permissions and `bin/*` stubs. Re-run it anytime to change presets; the previous configuration is replaced cleanly.

Four preset levels:

| Level | What it means |
|-------|--------------|
| **Fully autonomous** | All skills auto for both axes — zero prompts |
| **Fully manual** | Every diff reviewed, every phase transition confirmed |
| **Recommended** | Auto where safe (Review), manual where judgment matters (Code) |
| **Customize** | Choose per skill and per axis |

`/flow-prime` is a user-only command — the model never runs it on your behalf. That's intentional: priming mutates project config (`.claude/settings.json`, `bin/*` stubs, git excludes) that every engineer on the repo will inherit, so the decision belongs to a human.

### What prime writes to `.flow.json`

`/flow-prime` materializes your preset choice as JSON at `<project_root>/.flow.json`. The file is git-excluded — each engineer sets their own autonomy. You can hand-edit it if you want fine-grained per-skill control instead of re-running the preset picker:

```json
{
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto"
  }
}
```

View your current settings anytime with `/flow-config`. Run `/flow-prime` again to swap presets.

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
/flow-start #309
```

The argument must match `^#[1-9][0-9]*$` — a pre-decomposed GitHub issue number. The decomposed issue is the one `/flow-plan` produced: either a `#N` you passed to `/flow-plan` (in-place re-plan) or a new issue `/flow-plan` filed from a bare prompt. This acquires a start lock (serializing concurrent starts), pulls the integration branch, runs `bin/flow ci` for a clean baseline, upgrades dependencies on the integration branch, runs `bin/flow ci` again to catch dep-induced breakage, commits everything to the integration branch, then fetches the issue title to derive the branch name, creates a worktree at `.worktrees/<branch>`, and opens a GitHub PR. You land in Phase 2: Code.

---

## More Planning Tools

The three-command workflow handles single features. For bigger surfaces, FLOW adds two more shapes — whole-project decomposition, and working through an existing backlog.

Every planning skill is role-bound: PM, Tech Lead, or CTO voices with their own scope authority. Each persona refuses overreach with a `## SCOPE REFUSAL` block that names the next tier. No auto-escalation — the user always directs.

### Whole-project decomposition

- **`/flow-decompose-project <description>`** — Decompose a large project into a fully linked GitHub issue graph: epic, sub-issues, blocked-by dependencies, and phase labels. Every issue is filed work-ready with acceptance criteria, file paths, and scope boundaries from real codebase exploration. Feeds directly into `/flow-orchestrate`.

### Working an existing backlog

- **`/flow-issues`** — Group open issues by label into four sections (Blocked, Other, Vanilla, Decomposed) with mechanical sort and a copy-pasteable command per row.
- **`/flow-triage-issue #N`** — PM-lens triage of a single issue; verdict in `{close, decompose}` with confidence and flip-condition.
- **`/flow-orchestrate`** — Process every issue labeled "Decomposed" sequentially overnight via `flow-start --auto`. Progress and results land in `flow tui`'s Orchestration tab — completed flows with PR links, failed flows with reasons, total elapsed time.

---

## Skill Catalog

Run `/flow-skills` anytime to see the live catalog grouped by role.

### Planning

| Skill | Purpose |
|-------|---------|
| `/flow-issues` | Group open issues by label into four sections (Blocked, Other, Vanilla, Decomposed) with mechanical sort and a copy-pasteable command per row |
| `/flow-triage-issue` | PM-lens triage of a single open issue — verdict in {close, decompose} |
| `/flow-explore` | Open a problem-statement conversation (PM voice); file a vanilla `## What` / `## Why` / `## Acceptance Criteria` issue on signal |
| `/flow-plan` | Produce an implementation plan and attach it to an issue (Tech Lead voice). `#N` re-plans the existing issue in place; a bare topic synthesizes `## What` / `## Why` / `## Acceptance Criteria` and files a new decomposed issue |
| `/flow-decompose-project` | Decompose a large project into linked GitHub issues with sub-issue and blocked-by relationships |
| `/flow-orchestrate` | Process decomposed issues sequentially overnight via `flow-start --auto` |

### Work

| Skill | Purpose |
|-------|---------|
| `/flow-start` | Begin a new feature — worktree, PR, state file, plan extraction from issue body sentinels |
| `/flow-commit` | Show diff, write commit message, run CI gate, commit and push — used by every commit-producing skill |
| `/flow-note` | Capture a correction or learning to the active flow's state file mid-session |
| `/flow-config` | Display the per-skill autonomy configuration from `.flow.json` |
| `/flow-skills` | Display this catalog grouped by role |

### Health

| Skill | Purpose |
|-------|---------|
| `/flow-doc-sync` | Full codebase documentation accuracy review — reports drift between code and docs |
| `/flow-hygiene` | Audit instruction corpus health — `CLAUDE.md`, rules, and memory for staleness, duplication, and contradictions |

### Admin (user-only)

These commands are reserved for direct user invocation — type the slash command yourself. The model never invokes them on your behalf.

| Skill | Purpose |
|-------|---------|
| `/flow-prime` | One-time project setup — configure permissions, install `bin/*` stubs, write the version marker |
| `/flow-abort` | Abort the current feature — close the PR, delete the remote branch, remove the worktree, delete the state file |
| `/flow-continue` | Resume a halted autonomous flow — clears `_halt_pending` so the next assistant turn proceeds |
| `/flow-reset` | Reset all FLOW artifacts on this machine — close PRs, remove worktrees, delete branches, clear state files |

---

## Terminal Dashboard

Monitor every active flow from your terminal — no Claude session needed.

```bash
flow tui
```

Reads state files directly and auto-refreshes every 2 seconds, so phase transitions and code task progress appear as they happen. Runs standalone on macOS and Linux.

| Key | Action |
|-----|--------|
| Up/Down | Navigate flow list |
| Left/Right | Switch tab |
| Enter | Open worktree in terminal (activates existing iTerm2 tab or opens new tab) |
| p | Open PR in browser |
| i | Show issues list |
| I | Open issue in browser |
| t | Show tasks view |
| l | Show session log |
| a | Abort flow (with Y/N confirmation) |
| r | Force refresh |
| Esc | Back to list view |
| q | Quit |

The detail panel shows the full phase timeline with per-phase cumulative time, code task progress, diff stats, notes count, and issues filed.

---

## Architecture

### Sub-agents

Review launches four cognitively isolated agents in parallel:

- **`reviewer`** (context-rich) — receives diff + plan + `CLAUDE.md` + rules; covers architecture, simplicity, and correctness including security.
- **`pre-mortem`** (context-sparse) — receives only the substantive diff; investigates failure modes including security.
- **`adversarial`** (context-sparse) — writes tests designed to break the implementation.
- **`documentation`** (context-sparse) — assesses maintainability and documentation accuracy.

The parent session gathers context, triages findings, and fixes in-scope issues. Learn uses `learn-analyst` (cognitively isolated compliance audit). Planning skills can dispatch to `pm` / `tech-lead` / `cto` agents for scope-bound voices. Start uses `ci-fixer` when CI on the integration branch fails.

```text
Main conversation          Sub-agent (custom plugin)
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

### State file persistence

Every feature has a state file at `.flow-states/<branch>/state.json` containing identity (branch, repo, PR), phase tracking (current phase, timings, transitions), artifact paths (plan, dag, log), progress counters, notes captured via `/flow-note`, continuation and compaction recovery fields, autonomy settings, Slack thread info, issues filed, and diff stats. Full schema: [`docs/reference/flow-state-schema.md`](docs/reference/flow-state-schema.md).

State survives session breaks and compaction. Multiple features run simultaneously in separate worktrees with separate state files — both on the same machine and across multiple engineers.

### Session-start hook

Every Claude Code session start — new terminal, `/clear`, `/compact` — triggers a hook that scans `.flow-states/` for in-progress features. If one is found, Claude knows the feature name, current phase, worktree, and code task progress, but does not act on it.

The hook also handles timing recovery (resets interrupted session timing so cumulative phase durations stay accurate), compaction recovery (consumes `compact_summary` and `compact_cwd` for richer context after `/compact`), correction capture (injects the instruction to invoke `/flow-note` whenever the user corrects Claude), and deterministic terminal tab colors per repo.

### The learning system

Every correction and observation has a path to becoming a permanent, reusable pattern:

```text
User corrects Claude → /flow-note captures it in state["notes"]
Claude writes observations → auto-memory (shared across worktrees)
       ↓
Learn reads CLAUDE.md, rules, plan, state notes, and diff in Phase 4
       ↓
Each learning is routed to the right repo-local destination:
    → Project CLAUDE.md   (process rules and architecture — committed via PR)
    → Project rules       (coding anti-patterns and gotchas — committed via PR)
```

The learnings don't evaporate at session end. They compound.

### Slack notifications (optional)

Thread-per-feature notifications give your team passive awareness of feature progress. Each feature gets one Slack thread — every phase posts a reply, building a narrative from start to merge. Set two env vars, run `/flow-prime`, done. See [Slack Integration](docs/integrations/slack.md).

---

## What gets built per feature

Every completed feature produces:

- A merged PR with clean, TDD-tested, reviewed code
- Individual commits per plan task with detailed messages
- 100% test coverage maintained
- All identified risks addressed (verified by Review phase)
- New `CLAUDE.md` patterns and rules from corrections and learnings
- A clean state file (deleted at Complete)

---

## Updating

From the command line:

```bash
claude plugin marketplace update flow-marketplace
```

---

## License

[MIT](LICENSE)
