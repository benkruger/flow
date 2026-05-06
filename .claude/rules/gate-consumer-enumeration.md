# Gate Consumer Enumeration

When a plan introduces a new error reason, JSON field, or exit code to
an existing Rust subcommand whose stdout is parsed by skills, hooks,
or other subcommands, the plan must enumerate every consumer of that
output and require a matching error branch in the same PR.

A new error reason is a contract change. The producer (the
subcommand) starts emitting `{"status":"error","reason":"<new>"}` for
a previously-impossible situation. Every consumer that parses the
JSON must be updated to recognize the new reason — otherwise the
consumer silently ignores the error, treats the JSON as malformed,
or reads success fields off an error envelope and produces empty
output. The gate's protective intent is defeated.

## Why

A guard returns a structured error envelope so callers can detect
the failure cleanly instead of seeing shell corruption or random
empty output. That cleanliness only materializes if the caller
actually reads the `reason` field and branches on it. When the Plan
phase names tasks for the gate implementation but not for the
consuming skill's error-handling, the Code phase ships a producer
that emits the new reason and a consumer that silently drops it.
Code Review's pre-mortem agent then surfaces the gap as a
correctness bug and Step 4 fixes it. The cycle is avoidable: the
caller enumeration is cheap to write at Plan time, and writing it
forces the author to consider all consumers up front.

## The Rule

When a Plan-phase task adds or extends a Rust subcommand's error
output (new `reason` value, new `step` value, new exit code class,
or any new top-level JSON field that consumers branch on), the plan
must include — in the Tasks section, not the Risks section — a
**Consumer Enumeration Table** listing every reader of that output.
The table has four columns:

| Consumer | Output read | Current handling | Required change |
|---|---|---|---|
| `skills/flow-complete/SKILL.md` Step 6 | `complete-finalize` JSON | parses `formatted_time`, `summary`; ignores `status` | add `status=="error"` branch with retry-after-cd |
| `src/complete_fast.rs::run_impl` | `complete-finalize` JSON | does not invoke this subcommand | exempt — different caller chain |
| `agents/<name>.md` Output Format | (downstream agent prompt) | reads only `summary` | exempt — agent does not branch on errors |

Column definitions:

- **Consumer** — file path of the caller, plus the function or step
  that performs the parse.
- **Output read** — which subcommand's output the consumer parses.
- **Current handling** — how the consumer treats the existing JSON
  shape (which fields it reads, which it ignores).
- **Required change** — one of:
  - *Add a new branch* — consumer must branch on the new reason and
    handle it (retry, escalate, log, etc.). Code task description
    must spell out the branch's behavior.
  - *No change required* — consumer's existing handling already
    covers the new reason (e.g., a generic "ignore non-success"
    branch).
  - *Exempt* — consumer never invokes this subcommand or never
    reads this field.

## What Counts as a New Error Reason

A "new error reason" is any of:

- A new value in the `reason` field of the subcommand's error
  envelope (e.g., adding `"reason":"cwd_inside_worktree"` when
  prior versions only emitted `"reason":"cwd_drift"`).
- A new top-level JSON field that consumers parse (e.g., adding
  `post_merge_failures` when prior versions did not have one).
- A new exit code class beyond the established `0`/`1`/`2`
  convention (e.g., adding exit code `3` for a partial-success
  state).
- A new `status` value beyond `"ok"`/`"error"`/`"skipped"`
  (rare; usually a sign the schema is fragmenting).

Adding a new field that consumers do not parse (e.g., a debug
field, a new metric for the TUI, an internal hash) does not
trigger this rule — the field is invisible to existing
consumers.

## How to Enumerate

For every new error reason:

1. **Grep for callers of the subcommand.** Find every `bin/flow
   <name>` invocation in `skills/`, `.claude/skills/`,
   `hooks/`, `agents/`, and `src/`. The audit table has one row
   per callsite.
2. **Classify each consumer's parse.** Read
   the surrounding 5-10 lines to see which JSON fields the
   consumer reads. A consumer that captures `$(bin/flow ...)`
   into a variable and uses only `formatted_time` from it is
   reading `formatted_time`; it is NOT reading `status` or
   `reason`.
3. **For each consumer that does not currently branch on
   `status`/`reason`, add a Code-phase task** to extend the
   parse. The task description must name the branch's behavior
   (retry, escalate, propagate, etc.).
4. **Mark exempt consumers explicitly** — never leave a row
   blank. An exempt consumer named in the table proves the
   author considered it; an absent consumer is a Plan-phase
   gap.

## How to Apply

**Plan phase.** When a plan task adds a new error reason, JSON
field, or exit code class, write the Consumer Enumeration Table
in the same task description. The table belongs in the Tasks
section so the Code phase has it as a checklist. Each row that
requires a change becomes its own Code-phase task or atomic
group entry.

**Code phase.** Execute the producer change and every consumer
change in the same PR. A producer that emits a new reason and a
consumer that does not handle it is an internally-inconsistent
PR — Code Review's pre-mortem agent will catch it, but the
cheaper catch is at Plan time.

**Code Review phase.** The reviewer agent cross-checks every
new error reason in the diff against the consumer enumeration
table. A consumer that the table marked "Add a new branch" but
the diff did not extend is a Real finding fixed in Step 4.
Conversely, a new reason that has no enumeration table at all
is a Plan-phase gap — fix in Step 4 by writing the table
retroactively, then verifying every consumer is correct.

## Cross-References

- `.claude/rules/cli-output-contracts.md` — the broader rule for
  CLI output contracts (output format, exit codes, error
  messages, fallback behavior). This rule extends it: when a
  contract changes, every consumer must be enumerated.
- `.claude/rules/scope-enumeration.md` — sibling rule for
  universal-coverage claims. Both rules force enumeration before
  the Code phase begins; this rule is the error-handling
  dimension.
- `.claude/rules/code-review-scope.md` — Real findings fixed in
  the PR. A missing consumer-side update surfaced by Code Review
  is fixed in Step 4 regardless of which file it lives in.
- `.claude/rules/plan-commit-atomicity.md` — when the consumer
  update lives in a different file from the producer, the plan
  must mark the producer task and the consumer task as an
  atomic group so they land in the same commit.
