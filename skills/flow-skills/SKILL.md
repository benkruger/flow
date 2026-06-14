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
  FLOW v2.6.1 — flow:flow-skills — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Detect repo identity

This step decides whether the session is running inside the FLOW
plugin source repository itself. The decision gates two extra
sections (Maintainer and Private) that surface release tooling and
phase-internal helpers — content that only applies to FLOW plugin
development. Default-deny: the FLOW-only sections render only when
the repo identity match succeeds; every other case (including a
remote URL that is missing, malformed, or names a fork) is treated
as not the FLOW plugin source.

Run a single Bash call to read the configured remote URL:

```bash
git remote get-url origin
```

If the command exits non-zero, treat the repo as **not the FLOW
plugin source** and proceed to Step 2 with the FLOW-only sections
suppressed.

If the command exits zero, normalize stdout: strip trailing
whitespace and an optional trailing `.git` suffix. The repo is the
FLOW plugin source ONLY when the normalized URL ends with the
exact owner/repo path component `benkruger/flow` — equivalently,
when the URL matches one of these literal forms (plus optional
`.git`):

- `git@github.com:benkruger/flow`
- `https://github.com/benkruger/flow`
- `ssh://git@github.com/benkruger/flow`

A URL like `git@github.com:benkruger/flow-fork`, `git@github.com:benkruger/flow-experiments`, or `https://github.com/anyone/benkruger-flow-clone` MUST be treated as not the FLOW plugin source — the leading owner/repo segment must equal `benkruger/flow`, not merely contain it. A bare substring match would over-include forks and similarly-named repos.

### Step 2 — Render tables

This step prints the skill catalog directly in the response so
users can read it without leaving the conversation. The catalog is
grouped by user role — Planning, Work, Health, Admin — so the most
relevant skills surface first for the reader's context. The two
FLOW-repo-only sections appear at the bottom because they are
maintainer-internal: surfacing them in target projects would name
private skills the user cannot invoke.

Output the skill catalog as text in your response (not via Bash).
Always render Planning, Work, Health, and Admin. Render Maintainer
and Private only when Step 1 identified this repo as the FLOW
plugin source.

#### Planning

| Skill | Purpose |
|-------|---------|
| `/flow:flow-issues` | Group open issues by label into four sections (Blocked, Other, Vanilla, Decomposed) with mechanical sort and a copy-pasteable command per row |
| `/flow:flow-triage-issue` | PM-lens triage of a single open issue — verdict in {close, decompose} |
| `/flow:flow-explore` | Open a problem-statement conversation (PM voice) — discussion-mode by default, files a vanilla `## What` / `## Why` / `## Acceptance Criteria` issue on user signal |
| `/flow:flow-plan` | Decompose a vanilla problem-statement issue into a decomposed issue ready for the start phase (or one child decomposed issue per disconnected DAG component when multi-track applies per AC#4). Tech Lead voice, mandatory `decompose:decompose` pass, files with `--label decomposed` and links cross-component dependencies via `bin/flow link-blocked-by` |
| `/flow:flow-orchestrate` | Process decomposed issues sequentially overnight via flow-start |

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
| `/flow:flow-continue` | Resume a halted autonomous flow — clears `_halt_pending` so the next assistant turn proceeds |
| `/flow:flow-reset` | Wipe `.flow-states/` on this machine — PRs, worktrees, and branches are NOT touched (those need a per-flow abort) |

The Admin skills above are user-only: the model never invokes them
on your behalf. Type the slash command directly. Inside the FLOW
plugin source, the Maintainer skills below are also user-only.

The remaining sections render only when this repo is the FLOW
plugin source. If Step 1 identified the repo otherwise, stop here
and skip to the COMPLETE banner.

#### Maintainer

| Skill | Purpose |
|-------|---------|
| `/flow-qa` | File a pre-decomposed QA issue against the FLOW plugin repo for end-to-end lifecycle regression testing |
| `/flow-release` | Bump version in plugin.json and marketplace.json, commit, tag, push, and create a GitHub Release |
| `/flow-changelog-audit` | Audit the Claude Code CHANGELOG.md for plugin-relevant changes; categorize as Adopt/Remove/Adapt and file issues |

#### Private

| Skill | Invoked by | Purpose |
|-------|------------|---------|
| `/flow:flow-code` | Phase skill auto-chained from flow-start | Phase 2 — execute plan tasks one at a time with TDD |
| `/flow:flow-review` | Phase skill auto-chained from flow-code | Phase 3 — six tenants assessed by four cognitively isolated agents |
| `/flow:flow-complete` | Phase skill auto-chained from flow-review | Phase 4 — merge the PR, remove the worktree, delete the state file |
| `/flow:flow-commit` | Phase skill at every commit checkpoint | Review the full diff, then stage, commit, and push through finalize-commit |
| `/flow:flow-note` | Claude on user correction | Capture a correction or learning to the FLOW state file |

The Private skills are invoked by other FLOW skills or hooks, not
by the user directly.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.6.1 — flow:flow-skills — COMPLETE
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
