---
name: flow-skills
description: "Display the FLOW skill catalog grouped by user role. Maintainer and Private buckets render only when invoked inside the FLOW plugin repo."
---

# FLOW Skills — Available Commands

## Usage

```text
/flow:flow-skills
```

Display-only skill. Reads no state. Reports the FLOW skill catalog
segmented by user role. The Maintainer and Private sections render
only when the current repo is the FLOW plugin source.

## Concurrency

Read-only and concurrent-safe. The skill touches no state files,
acquires no locks, and reads only the local git remote URL. Multiple
invocations on the same machine or across machines do not interact.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-skills — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Detect repo identity

Run a single Bash call to read the configured remote URL:

```bash
git remote get-url origin
```

If the command exits non-zero, treat the repo as **not the FLOW
plugin source** and proceed to Step 2 with the FLOW-only sections
suppressed.

If the command exits zero, inspect stdout. The repo is the FLOW
plugin source when the URL contains the literal substring
`benkruger/flow` (with or without a trailing `.git`). Any other
URL — including forks under different owners and unrelated
projects — is treated as not the FLOW plugin source.

### Step 2 — Render tables

Output the skill catalog as text in your response (not via Bash).
Always render Planning, Work, Health, and Admin. Render Maintainer
and Private only when Step 1 identified this repo as the FLOW
plugin source.

#### Planning

| Skill | Purpose |
|-------|---------|
| `/flow:flow-issues` | Fetch open issues, rank by impact, and display a dashboard with recommended work order |
| `/flow:flow-triage-issue` | PM-lens triage of a single open issue — verdict in {close, decompose, keep-open, fix-now} |
| `/flow:flow-create-issue` | Capture a brainstormed solution as a pre-planned issue with an Implementation Plan section |
| `/flow:flow-decompose-project` | Decompose a large project into linked GitHub issues with sub-issue and blocked-by relationships |
| `/flow:flow-orchestrate` | Process decomposed issues sequentially overnight via flow-start --auto |

#### Work

| Skill | Purpose |
|-------|---------|
| `/flow:flow-start` | Begin a new feature — worktree, PR, state file, plan extraction from issue body sentinels |
| `/flow:flow-config` | Display the per-skill autonomy configuration from `.flow.json` |
| `/flow:flow-skills` | Display this skill catalog grouped by user role |

#### Health

| Skill | Purpose |
|-------|---------|
| `/flow:flow-doc-sync` | Full codebase documentation accuracy review — reports drift between code and docs |
| `/flow:flow-hygiene` | Audit instruction corpus health — CLAUDE.md, rules, and memory for staleness, duplication, and contradictions |

#### Admin

| Skill | Purpose |
|-------|---------|
| `/flow:flow-prime` | One-time project setup — configure permissions, install bin/* stubs, write the version marker |
| `/flow:flow-abort` | Abort the current feature — close the PR, delete the remote branch, remove the worktree, delete the state file |
| `/flow:flow-reset` | Reset all FLOW artifacts on this machine — close PRs, remove worktrees, delete branches, clear state files |

These four are user-only: the model never invokes them on your
behalf. Type the slash command directly.

The remaining sections render only when this repo is the FLOW
plugin source. If Step 1 identified the repo otherwise, stop here
and skip to the COMPLETE banner.

#### Maintainer

| Skill | Purpose |
|-------|---------|
| `/flow-release` | Bump version in plugin.json and marketplace.json, commit, tag, push, and create a GitHub Release |

#### Private

| Skill | Invoked by | Purpose |
|-------|------------|---------|
| `/flow:flow-code` | Phase skill auto-chained from flow-start | Phase 2 — execute plan tasks one at a time with TDD |
| `/flow:flow-code-review` | Phase skill auto-chained from flow-code | Phase 3 — six tenants assessed by four cognitively isolated agents |
| `/flow:flow-learn` | Phase skill auto-chained from flow-code-review | Phase 4 — capture learnings, route to permanent homes |
| `/flow:flow-complete` | Phase skill auto-chained from flow-learn | Phase 5 — merge the PR, remove the worktree, delete the state file |
| `/flow:flow-commit` | Phase skill at every commit checkpoint | Review the full diff, then stage, commit, and push through finalize-commit |
| `/flow:flow-status` | Phase skill at manual-mode handoffs | Print the status panel — phase timeline, PR link, what comes next |
| `/flow:flow-note` | Claude on user correction | Capture a correction or learning to the FLOW state file |

The Private skills are invoked by other FLOW skills or hooks, not
by the user directly.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-skills — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Display only — never modify any file or state
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- Never compute time, counters, or timestamps — this skill performs no state mutation
