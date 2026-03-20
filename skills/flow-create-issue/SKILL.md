---
name: flow-create-issue
description: "Decompose a problem via DAG analysis with deep codebase exploration, iterate with the user until the issue is 100% ready for autonomous execution, then file it."
---

# Flow Create Issue

Decompose a problem into a fully detailed, work-ready GitHub issue. Uses the `decompose:decompose` plugin for DAG-based analysis with deep codebase exploration. Iterates with the user until the issue is comprehensive enough for `/flow:flow-start` to execute it fully autonomously — no human clarification needed.

## Usage

```text
/flow:flow-create-issue <problem description>
/flow:flow-create-issue --step 2 <problem description>
/flow:flow-create-issue --step 3 <problem description>
/flow:flow-create-issue --step 4 <problem description>
```

- `/flow:flow-create-issue <problem description>` — start from Step 1 (Decompose)
- `/flow:flow-create-issue --step 2` — self-invocation: skip to Step 2 (Draft)
- `/flow:flow-create-issue --step 3` — self-invocation: skip to Step 3 (Review)
- `/flow:flow-create-issue --step 4` — self-invocation: skip to Step 4 (File)

## Concurrency

This skill creates shared GitHub state (issues). Issue creation is
idempotent by title — if an issue with the same title already exists,
the user should be warned before filing a duplicate.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v0.36.2 — flow:flow-create-issue — STARTING
──────────────────────────────────────────────────
```
````

## Step Dispatch

If `--step N` was passed, this is a self-invocation from a previous step.
Skip the Announce banner and jump directly to Step N.

- `--step 2` → jump to Step 2
- `--step 3` → jump to Step 3
- `--step 4` → jump to Step 4

If no `--step` flag was passed, proceed to Step 1.

---

## Step 1 — Decompose

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 1 of 4: Decompose ──
```
````

Invoke the `decompose:decompose` plugin with the user's problem description via the Skill tool.

The decomposition **must** include deep codebase exploration. During DAG execution:

- Use **Glob** to find relevant files by pattern
- Use **Grep** to search for related code, constants, error messages, and patterns
- Use **Read** to understand current behavior, architecture, and constraints
- Trace call chains and dependencies to identify all affected files
- Verify that referenced files, functions, and patterns actually exist

The decomposition is the foundation. Every claim in the final issue must be grounded in evidence from the codebase — not theoretical or assumed. If decompose produces findings that reference files or behavior, verify them.

Present the full DAG synthesis to the user.

<HARD-GATE>

Ask the user to review the decomposition using AskUserQuestion:

- **"Proceed to draft"** → invoke `flow:flow-create-issue --step 2` using the Skill tool as your final action. Do not output anything else after this invocation.
- **"Iterate on decomposition"** → re-invoke `decompose:decompose` with the user's feedback, present the updated synthesis, and ask again.
- **"Cancel"** → stop. Do not file an issue.

Do not proceed to Step 2 without explicit user approval.

</HARD-GATE>

---

## Step 2 — Draft Issue

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 2 of 4: Draft ──
```
````

Take the decompose synthesis and craft a single GitHub issue. The issue must contain enough detail that a fresh Claude session running `/flow:flow-start work on issue #N` can execute it fully autonomously — no questions asked.

### Required Sections

**Problem** — What is broken, missing, or inadequate. Include observable behavior, evidence from the codebase (file paths, line numbers, code snippets), and user impact. This is not a guess — it is grounded in the codebase exploration from Step 1.

**Acceptance Criteria** — A checklist of binary, testable conditions. Each criterion must be pass/fail with no subjective judgment. These become the definition of done for the autonomous session.

Example:

```text
- [ ] `lib/session-start.sh` detects multiple active features and lists them
- [ ] `bin/ci` passes with no new warnings
- [ ] Test coverage for the new detection logic in `tests/test_session_start.py`
```

**Files to Investigate** — Real file paths verified during codebase exploration. These are starting points for the Plan phase. Include a brief note on why each file is relevant.

Example:

```text
- `lib/session-start.sh` — contains the hook logic that needs modification
- `tests/test_session_start.py` — existing tests to extend
- `hooks/hooks.json` — hook registration that may need updating
```

**Out of Scope** — Explicit boundaries to prevent autonomous scope creep. Name specific things that should NOT be changed, even if they seem related. This is critical for autonomous execution — without it, Claude will "improve" adjacent code.

**Context** — Business reason, architectural constraints, or design decisions that inform the implementation. Include anything a fresh session needs to understand *why* this work matters and *how* it fits into the broader system.

### Draft Presentation

Present the full draft inline in the response — both title and body. Do not tell the user to look at a file. Render it as a formatted markdown block so the user can review every detail.

<HARD-GATE>

Ask the user to review the draft using AskUserQuestion:

- **"File it"** / **"Looks good"** / **"Ship it"** → invoke `flow:flow-create-issue --step 4` using the Skill tool as your final action. Do not output anything else after this invocation.
- **"Revise the draft"** / **Any feedback or change request** → revise the draft based on feedback and re-present. After revision, ask again with the same options.
- **"Re-decompose"** → invoke `flow:flow-create-issue --step 1` using the Skill tool as your final action. Do not output anything else after this invocation.

The issue must not be filed without explicit user approval.

</HARD-GATE>

---

## Step 3 — Review

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 3 of 4: Review ──
```
````

<HARD-GATE>

This is a mandatory approval gate. The issue must not be filed without explicit user approval.

Present the full draft inline in the response. Ask the user to review the draft using AskUserQuestion:

- **"File it"** / **"Looks good"** / **"Ship it"** → invoke `flow:flow-create-issue --step 4` using the Skill tool as your final action. Do not output anything else after this invocation.
- **Any feedback or change request** → Revise the draft and re-present. If the feedback is substantial (changes the problem understanding or approach), re-run `decompose:decompose` with the updated understanding. If the feedback is editorial (wording, scope adjustments), revise the draft directly. After revision, ask again.

Iterate as many times as needed. There is no shortcut. The issue is not filed until the user explicitly approves.

</HARD-GATE>

---

## Step 4 — File

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 4 of 4: File ──
```
````

Write the issue body to `.flow-issue-body` in the project root using the Write tool, then file it:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body --label decomposed
```

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

Display the issue URL to the user, then output the COMPLETE banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.36.2 — flow:flow-create-issue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never file an issue without explicit user approval — Step 2 and Step 3 are mandatory gates
- Never skip codebase exploration — every file path and code reference must be verified
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- The issue body must be self-contained — a fresh session with no memory of this conversation must be able to execute it
- Never create sub-issues or linked issues — file a single comprehensive issue
- Always use the Write tool to create `.flow-issue-body` — never pass body text as a CLI argument
- Never delete `.flow-issue-body` — the `bin/flow issue` script handles cleanup
- Each step ends by invoking the skill itself as the final action — never continue to the next step in the same invocation
