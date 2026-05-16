---
name: flow-qa
description: "File a pre-decomposed QA issue against the FLOW plugin repository for end-to-end lifecycle regression testing."
---

# FLOW QA

Maintainer-only skill that files a pre-decomposed QA issue against the
FLOW plugin repo. The issue describes a non-destructive Code-phase
change to `hello.sh` — the designated smoke-test artifact — so the
maintainer can exercise the full Start → Code → Review → Learn →
Complete lifecycle against a low-risk target.

## Announce

Print the banner block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.2.0 — flow-qa — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Write utility marker

Write the per-session utility-in-progress marker so the multi-step
filing pipeline survives any mid-skill sub-agent return:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow-qa
```

## Step 2 — Derive identifiers

Generate a short session ID and capture today's date with both
clock-bearing and clock-free forms. Run all three in parallel via
the Bash tool:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id
```

```bash
date "+%Y-%m-%d %H-%M"
```

```bash
date "+%Y-%m-%d"
```

Capture the outputs as `<id>`, `<title_date>`, and `<plan_date>`
respectively. The clock fragment in `<title_date>` makes same-minute
re-runs recognizable as duplicate-title filings; the `<plan_date>`
without clock is the value embedded in the issue body's smoke-test
greeting so QA passes filed within the same day produce identical
plan content.

## Step 3 — Compose issue body

Compose the issue body in working memory with this exact shape:

````markdown
## What

A non-destructive smoke-test pass against the FLOW plugin's
`hello.sh` artifact. The Code phase updates the greeting to record
the QA date so the full Start → Code → Review → Learn → Complete
lifecycle exercises a low-risk file.

## Why

`hello.sh` is the designated smoke-test artifact — referenced only
by `CLAUDE.md`, with no callers, no tests, and no coverage impact.
Routing a fresh QA pass through the standard lifecycle on this file
confirms every phase still operates against the current FLOW
plugin source.

## Acceptance Criteria

- `hello.sh` line 2 reads `echo "Hello, FLOW! (QA <plan_date>)"`
  after the Code phase.
- `tests/hello_smoke.rs` exists and asserts `hello.sh` content
  contains the literal substring `echo "Hello, FLOW! (QA
  <plan_date>)"`.
- `bin/flow ci` passes green on the feature branch.

<!-- FLOW-PLAN-BEGIN -->
## Implementation Plan

### Context

`hello.sh` is the FLOW plugin's smoke-test artifact for full
lifecycle regression passes. Each QA pass replaces line 2 of the
script with a fresh date-stamped greeting and ships a single
integration test that asserts the greeting matches. The change is
idempotent — every pass overwrites both files with the new date,
producing a clean mergeable diff.

### Exploration

- **`hello.sh`** — 2-line bash script. Line 1 is the shebang; line
  2 prints the current greeting. No coverage impact, no callers.
- **`tests/hello_smoke.rs`** — new (or overwritten) integration
  test. Single function that reads `hello.sh` and asserts the
  greeting substring is present. Mirrors the project's
  `tests/<name>.rs` convention; auto-discovered by cargo.

### Risks

- **Same-minute re-runs.** The issue title carries an `HH-MM` clock
  fragment so duplicate-title detection surfaces visibly.
- **CI sentinel state.** `bin/flow ci` runs the full toolchain;
  the only file changes affect `hello.sh` and the smoke test, so
  no other test should regress.

### Approach

One TDD pair lands the smoke test plus the greeting update in one
commit. The test reads the script and asserts the date-stamped
greeting substring; the implementation updates the script to match.

### Dependency Graph

| Task | Type | Depends On |
|------|------|------------|
| 1. Write `tests/hello_smoke.rs` smoke test | test | — |
| 2. Update `hello.sh` line 2 to the date-stamped greeting | implement | 1 |

### Tasks

#### Task 1: Write the smoke test

Create `tests/hello_smoke.rs` containing one integration test that
reads `hello.sh` from `common::repo_root()` and asserts the file
content contains the literal substring `echo "Hello, FLOW! (QA
<plan_date>)"`.

Files: `tests/hello_smoke.rs`

#### Task 2: Update `hello.sh` line 2

Replace line 2 of `hello.sh` with:

```bash
echo "Hello, FLOW! (QA <plan_date>)"
```

Files: `hello.sh`

<!-- FLOW-PLAN-END -->
````

Substitute the literal `<plan_date>` value captured in Step 2 into
every occurrence inside the body.

## Step 4 — Write, validate, file, record, report

**Write.** Write the composed body to `.flow-issue-body-<id>` using
the Write tool. Use the worktree-relative form `(./)` when invoked
inside an active FLOW worktree, otherwise the bare relative form
resolves against the main repo root.

**Validate.** Validate the body file via the pre-filing checker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --mode decomposed --body-file .flow-issue-body-<id>
```

Parse the last line as JSON. If `status` is `error`, fix the body
per the named defect, rewrite the file, and re-run the validator
(max 5 attempts). After 5 failed attempts, halt and report.

**File.** Once `status` is `ok`, file against `benkruger/flow` with
the `decomposed` label and assignee `@me`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "FLOW QA Pass <title_date>" --body-file .flow-issue-body-<id> --label decomposed --assignee @me --repo benkruger/flow
```

Capture the returned issue URL and parse the trailing issue number
as `<M>`.

**Record.** Append the filed issue to the state file (no-op when
invoked outside an active flow):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "FLOW QA Pass <title_date>" --url "<issue_url>" --phase flow-qa
```

**Clear marker.** Remove the per-session utility marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow-qa
```

**Report.** Print the COMPLETE banner naming the issue URL and the
next command. Output in your response (not via Bash) inside a fenced
code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.2.0 — flow-qa — COMPLETE
  Filed: <issue_url>
  Next: /flow-start #<M>
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never edit `hello.sh` or `tests/hello_smoke.rs` from this skill —
  those file changes belong to the filed issue's Code phase, not
  to flow-qa itself.
- Never auto-invoke `/flow-start`; the maintainer types
  `/flow-start #<M>` after reviewing the filed issue.
- Always file with `--label decomposed --assignee @me --repo
  benkruger/flow`. The label routes the issue through the
  pre-decomposed path; the assignee surfaces the QA pass to the
  invoking maintainer; the repo flag pins filing to the FLOW
  plugin even when the user is on another project.
- Always validate with `--mode decomposed` before filing. The
  pre-filing validator catches malformed sentinels, missing
  `## Implementation Plan` heading, or empty task lists before
  the issue lands.
