---
name: research
description: "Phase 2: Research — explore the codebase before any design or implementation. Reads affected files, discovers risks, asks clarifying questions, and documents findings in flow-state.json."
model: sonnet
---

# FLOW Research — Phase 2: Research

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.1.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 1: Start must be
     complete. Run /flow:start first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.5 — Phase 2: Research — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 2:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if currently null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `2`

## Logging

No logging for this phase. Research runs no Bash commands beyond the entry
gate — the sub-agent runs in its own context and the main skill's work is
AskUserQuestion calls and state file writes.

---

## Step 1 — What are we researching?

Before reading any code, ask the user what to focus on. The feature name
from `/flow:start` is just a branch label — it does NOT define the
research scope. The user must describe what to research in their own words.

If this is a return visit (`visit_count` > 1), show what was previously
found and ask: "What gaps should we fill this time?" Do not discard prior
findings — extend them.

Otherwise, use AskUserQuestion with two questions:

**Question 1:** "What type of work is this?"
- New feature
- Change to existing feature
- Bug investigation
- Refactor / restructure

**Question 2:** "Describe what we should research. What area of the codebase should we explore, and what are we trying to understand?"
- I'll describe it (select Other and type your description)
- I'm not sure yet — help me figure out where to start

The user's answer to Question 2 directs the entire exploration. If they
select "I'm not sure yet", ask follow-up questions to narrow the scope
before proceeding. Do not assume scope from the feature branch name.

Store the user's description in `state["research"]["scope"]` in the
state file.

---

## Step 2 — Launch codebase explorer sub-agent

Launch a mandatory sub-agent to explore the codebase. Use the Task tool:

- `subagent_type`: `"Explore"`
- `description`: `"Research codebase exploration"`

Provide these instructions to the sub-agent (fill in the scope from Step 1):

> You are exploring a Rails codebase for the FLOW research phase.
> Research scope: <user's description from Step 1 — paste verbatim>
>
> **Tool rules:** Use Glob and Read tools for all file and directory checks.
> Use Grep for searching code. Never use Bash for file existence checks,
> directory listings, or reading file contents (`test -f`, `ls`, `cat`, etc.).
>
> Systematically read all code relevant to this feature:
>
> **Models** — Find all related models. For each, read the full class
> hierarchy (model + parent + ApplicationRecord). Look for: before_save,
> after_create, before_destroy callbacks. Check for default_scope (soft
> deletes), self.inheritance_column (no STI), belongs_to/has_many with
> dependent: options. Note the Base/Create split pattern.
>
> **Controllers** — Find affected controllers. Note subdomain, BaseController
> inheritance, params pattern (options OpenStruct), response helpers.
>
> **Workers** — Find affected Sidekiq workers. Read pre_perform!/perform!/
> post_perform! structure. Check config/sidekiq.yml for queue names.
>
> **Routes** — Read config/routes/ files relevant to this feature. Note
> scope with module:, as:, controller:, action: pattern.
>
> **Schema** — Read data/release.sql for all relevant tables. Note column
> types, constraints, indexes, foreign keys.
>
> **Tests** — Search test/support/ for existing create_*! helpers for
> affected models. Note existing test patterns.
>
> **Git history** — Run git log --oneline -10 on key files. Use git blame
> on anything non-obvious.
>
> Return your findings as a structured summary:
>
> - Affected files (full paths)
> - Per-model: class hierarchy, callbacks, associations, soft deletes
> - Per-controller: subdomain, BaseController, params pattern
> - Per-worker: queue name, halt conditions
> - Routes: file and pattern
> - Schema: table structure
> - Test helpers: existing create_*! helpers found
> - Risks: anything that could cause problems (callback chains, soft
>   deletes, Current attribute dependencies)

Wait for the sub-agent to return before proceeding. Use the sub-agent's
findings as the basis for all subsequent steps — do not re-read files
the sub-agent already covered.

---

## Step 3 — Formulate clarifying questions

Based on exploration, identify everything that is ambiguous or unclear about the feature. Write down all questions before presenting them.

Good research questions:
- Scope boundaries ("Does this apply to all accounts or just paying ones?")
- Edge cases ("What happens if the webhook arrives twice?")
- Existing behaviour ("Should this replace the current X or run alongside it?")
- Constraints ("Are there rate limits we need to respect?")
- Rollback ("What's the behaviour if this fails mid-way?")

Do NOT ask about things that can be inferred from the codebase. Only ask when genuinely unclear.

---

## Step 4 — Ask clarifying questions

Group questions into batches of up to 4. Present each batch using `AskUserQuestion` — the tab UI allows the user to navigate freely between questions with arrow keys.

For each batch, use a single `AskUserQuestion` call with up to 4 questions. Each question should have 2–4 options where possible (multiple choice is easier to answer than open-ended). Always include an "Other / I'll explain" option implicitly via the tool's built-in Other option.

If answers from the first batch reveal new questions, present a second batch.

Record every question and answer in `flow-state.json["research"]["clarifications"]`:

```json
[
  { "question": "What happens to existing webhooks when...", "answer": "They should be..." }
]
```

---

## Step 5 — Document findings

Write the full research findings into `flow-state.json["research"]`:

```json
{
  "research": {
    "clarifications": [...],
    "affected_files": [
      "app/models/payment/base.rb",
      "app/models/payment/create.rb",
      "app/workers/payment_webhook_worker.rb",
      "app/controllers/api/payments_controller.rb",
      "config/routes/api.rb",
      "data/release.sql",
      "test/support/payment_helpers.rb"
    ],
    "risks": [
      "Payment::Base has a before_save callback that sets Current.account — passing account explicitly in update! will be silently overwritten",
      "PaymentWebhookWorker queue is 'critical' in sidekiq.yml — any new worker for this feature should use the same queue",
      "Payments use soft deletes — queries must use .unscoped if deleted records are relevant"
    ],
    "open_questions": [
      "Stripe webhook signing secret — confirmed available in ENV but not yet in credentials"
    ],
    "summary": "The payment webhook system will touch three models (Payment::Base, Payment::Create, WebhookEvent::Create), one new worker, and a new API route. The most significant risk is the before_save callback on Payment::Base that sets processed_at from Current — this must be set via Current, not passed directly."
  }
}
```

---

## Step 6 — Present findings

Show the user a clean summary. Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW — Phase 2: Research — FINDINGS
============================================

  Affected Files
  --------------
  - app/models/payment/base.rb
  - app/workers/payment_webhook_worker.rb
  - ... (all files)

  Risks Discovered
  ----------------
  - Payment::Base before_save sets processed_at from Current
  - ...

  Open Questions
  --------------
  - Stripe webhook signing secret location

  Summary
  -------
  <summary text>

============================================
```
````

---

## Done — Update state and complete phase

Update `.flow-states/<branch>.json`:
1. Calculate `cumulative_seconds`: `current_time - session_started_at` + existing `cumulative_seconds`. Do not print the calculation.
2. Set Phase 2 `status` to `complete`
3. Set Phase 2 `completed_at` to current UTC timestamp
4. Set Phase 2 `session_started_at` to `null`
5. Set `current_phase` to `3`

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.8.5 — Phase 2: Research — COMPLETE (<formatted_time>)
  Next: Phase 3: Design  (/flow:design)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 2: Research is complete. Ready to begin Phase 3: Design?"
>
> - **Yes, start Phase 3 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 3 now" and "Not yet"

**If Yes** — invoke the `flow:design` skill using the Skill tool.

**If Not yet** — print inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Never propose a solution during Research — that is Design's job
- Never write or modify any application code
- Always read the full class hierarchy for every affected model — never just the model file
- Always check `test/support/` for existing helpers before noting that tests will be needed
- If returning to Research, read prior findings first and extend — never discard
