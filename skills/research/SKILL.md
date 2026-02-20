---
name: research
description: "Phase 2: Research — explore the codebase before any design or implementation. Reads affected files, discovers risks, asks clarifying questions, and documents findings in ror-state.json."
---

# ROR Research — Phase 2: Research

## Announce

Print:

```
============================================
  ROR — Phase 2: Research — STARTING
============================================
```

## Update State

Read `.claude/ror-state.json`. Update Phase 2:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if currently null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `2`

Update the Phase 2 task to `in_progress`.

---

## Step 1 — Read the feature context

Read `.claude/ror-state.json` to understand:
- The feature name and description
- Whether this is a return visit (check `visit_count` and any existing `research` data)

If returning to Research, read the previous findings in `ror-state.json["research"]` and note what was already discovered. Do not discard prior findings — extend them.

---

## Step 2 — Explore the codebase

Systematically read all code relevant to this feature. Work through each area below. Do not skip any that could be relevant.

### Models
- Find all models related to the feature domain
- For each model, read the **full class hierarchy**: the model itself, its parent (e.g. `ApplicationRecord`, `DigitalApplicationRecord`), and `ApplicationRecord`
- Look for: `before_save`, `after_create`, `before_destroy` and all other callbacks
- Look for: `default_scope` (soft deletes)
- Look for: `self.inheritance_column = :_type_disabled` (no STI)
- Look for: `belongs_to`, `has_many` with `dependent:` options
- Note the Base/Create split pattern

### Controllers
- Find affected controllers
- Note the subdomain each belongs to (each subdomain has its own BaseController)
- Note params pattern (`options` OpenStruct) and response helpers (`render_ok`, `render_error`)

### Workers
- Find affected Sidekiq workers
- Read `pre_perform!`, `perform!`, `post_perform!` structure
- Check `config/sidekiq.yml` for queue names

### Routes
- Read `config/routes/` files relevant to this feature
- Note the `scope` with `module:`, `as:`, `controller:`, `action:` pattern

### Schema
- Read `data/release.sql` for all tables relevant to this feature
- Note column types, constraints, indexes, foreign keys

### Tests
- Search `test/support/` for existing `create_*!` helpers for affected models
- Note existing test patterns — do not invent new patterns when helpers exist

### Git history
- Run `git log --oneline -10 -- <affected_files>` on key files
- If anything looks non-obvious, run `git blame` to understand why it exists

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

Record every question and answer in `ror-state.json["research"]["clarifications"]`:
```json
[
  { "question": "What happens to existing webhooks when...", "answer": "They should be..." }
]
```

---

## Step 5 — Document findings

Write the full research findings into `ror-state.json["research"]`:

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

Show the user a clean summary:

```
============================================
  ROR — Phase 2: Research — FINDINGS
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

---

## Step 7 — Phase gate

Ask the user:

> "Phase 2: Research is complete. Ready to proceed to Phase 3: Design?"
> - **Yes, proceed**
> - **No, keep researching**

If "No, keep researching" — ask what area still needs investigation and loop back to Step 2.

---

## Done — Update state and complete phase

Update `.claude/ror-state.json`:
1. Calculate `cumulative_seconds`: `current_time - session_started_at` + existing `cumulative_seconds`
2. Set Phase 2 `status` to `complete`
3. Set Phase 2 `completed_at` to current UTC timestamp
4. Set Phase 2 `session_started_at` to `null`
5. Set `current_phase` to `3`

Update the Phase 2 task to `completed`.
Update the Phase 3 task to `pending` (it already is, but confirm).

Print:

```
============================================
  ROR — Phase 2: Research — COMPLETE
  Next: Phase 3: Design  (/ror:design)
============================================
```

---

## Hard Rules

- Never propose a solution during Research — that is Design's job
- Never write or modify any application code
- Always read the full class hierarchy for every affected model — never just the model file
- Always check `test/support/` for existing helpers before noting that tests will be needed
- If returning to Research, read prior findings first and extend — never discard
