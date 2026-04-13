# External-Input Audit Gate

When a Plan-phase plan proposes tightening a function with a new
`assert!`, `panic!`, `assert_eq!`, `assert_ne!`, or constructor-level
invariant check on an input parameter, the plan must include a
callsite source-classification audit table covering all of the
function's callers. Without the table, the plan can assert
"upstream sanitization guarantees X" when the assumption is wrong
— and the resulting panic crashes any user whose caller sources
the value from outside the process.

This rule is the mechanical complement to
`.claude/rules/external-input-validation.md` (the prose discipline)
— that rule describes WHY the audit matters; this rule describes
HOW the gate enforces it at Plan-phase completion.

## Why

Issue #1054 surfaced this exactly. The plan tightened
`FlowPaths::new` on the assumption that `branch_name()` sanitization
applied to the function's callers. The assumption was wrong:
`branch_name()` only runs at flow-start, while `current_branch()`
returns raw git refs that commonly contain slashes
(`feature/foo`, `dependabot/*`). Five hooks and `format-status`
crashed with a Rust panic for users on standard git branches.
The adversarial agent caught the regression in Code Review — too
late; the cheaper catch is at Plan time.

The rule exists because:

1. **Reviewers cannot manually catch every callsite assumption.**
   PR #1054's plan went through Code Review and only the
   adversarial agent (writing failing tests) noticed the gap.
2. **The audit is cheap to write.** A four-column table per
   panic-introducing plan is on the order of minutes; the cost of
   a missed audit is one full Code Review cycle plus a hot-fix.
3. **Force-functioning the conversation works.** Requiring the
   table at Plan completion forces the author to enumerate
   callers — which is the same activity that catches the
   `current_branch()` assumption directly.

## The Trigger Vocabulary

The gate fires on plan prose containing one of:

- **Verb + panic-class noun** — one of the verbs
  `add` / `tighten` / `introduce` / `enforce` / `require` / `impose`
  followed by one of the nouns `panic` / `assert!` / `assert_eq!` /
  `assert_ne!` / `panic!` / `invariant check` /
  `validation assertion` / `constructor invariant`. Optional
  adjectives like "new" between the verb and noun are tolerated.
- **Direct action phrase** — `panic on <word>`, `assert that
  <word>`, or `reject (empty|invalid|malformed|unsupported)
  <word>`.
- **Direct macro mention** — bare `assert!`, `panic!`,
  `assert_eq!`, or `assert_ne!` macro invocations (with the open
  parenthesis attached) outside fenced code blocks. Plan prose
  that literally includes those tokens is almost always a proposal
  to add them.

The vocabulary is closed and curated — novel phrasings that slip
past the regex are handled by extending the vocabulary in
follow-up commits, mirroring the discipline documented for
`.claude/rules/scope-enumeration.md` and
`.claude/rules/comment-quality.md`. The rule file is the primary
instrument; this scanner is the merge-conflict trip-wire.

When a reviewer finds a novel phrasing that should have been
caught, add it to `TRIGGER_PATTERN` (or `DIRECT_TOKEN_PATTERN`) in
`src/external_input_audit.rs`, add a matching trigger unit test,
update the vocabulary list above, and note the addition in the
commit message.

## Compliance Proof — The Audit Table

The gate looks for a Markdown table within
`WINDOW_NON_BLANK_LINES` of the trigger that names all four
required columns. The header row may use these aliases:

| Required column | Accepted headers |
|------|------|
| Caller | `caller`, `callsite` |
| Source | `source` |
| Classification | `classification`, `class`, or any header starting with `classif` |
| Handling | `handling`, `disposition` |

The table accepts the standard Markdown table forms — with or
without leading/trailing pipes, with or without alignment
markers (`:---`, `:---:`, `---:`), with extra intra-cell
whitespace.

A canonical audit table looks like:

| Caller | Source | Classification | Handling |
|--------|--------|----------------|----------|
| `current_branch()` (`src/git.rs`) | git subprocess output | Trusted-but-external | `try_new`, treat `None` as no-active-flow |
| `state["branch"]` (`src/start_init.rs`) | state file written by `branch_name()` | Guaranteed valid | `new` directly |

The gate validates **table presence**, not row content. The rule's
authority validates rows; the gate is a forcing function for the
audit conversation. A TBD-content table (`| TBD | TBD | TBD | TBD |`)
will pass the gate, but it signals author irresponsibility to the
reviewer — that signal is the rule's responsibility, not the
gate's.

## Opt-Out Grammar

Plan prose that mentions `panic`/`assert!` in discussion (not as a
tightening proposal) can carry the opt-out comment
`<!-- external-input-audit: not-a-tightening -->` on:

- the trigger line itself (same-line, anywhere on the line),
- the line directly above the trigger, or
- two lines above with a single blank line in between.

Larger gaps do not chain — the rule is "the next non-blank line
with at most one blank line separating them," matching the
sibling opt-out grammar in `scope-enumeration.md`.

The opt-out walks back exactly one blank line by design — a stray
opt-out at the top of a section cannot silence arbitrary triggers
further down.

## Enforcement Topology

Three callsites share `external_input_audit::scan`:

- **Standard plan path** — `bin/flow plan-check` (called from
  `skills/flow-plan/SKILL.md` Step 4 →
  `src/plan_check.rs::run_impl`). This is the gate the model hits
  for plans it writes from scratch.
- **Pre-decomposed extracted path** —
  `src/plan_extract.rs` extracted path (~line 668) runs the
  scanner against the promoted plan content for issues filed via
  `/flow:flow-create-issue` with an `## Implementation Plan`
  section.
- **Resume path** — `src/plan_extract.rs` resume path (~line 432)
  re-runs the scanner against the existing plan file when the
  user re-enters Phase 2 after a prior violation. A plan edited
  to fix the violations passes the gate here and the phase
  completes.

All three callsites return the same JSON error shape
(`status="error"`, `violations[]`, `message`) so the repair loop
is identical regardless of which path triggered the failure. Each
violation carries a `rule` field (`"scope-enumeration"` or
`"external-input-audit"`) so the author knows which rule file to
consult for the fix.

A contract test in `tests/external_input_audit.rs` covers the
committed prose corpus (`CLAUDE.md`, `.claude/rules/*.md`,
`skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) so future
regressions in those surfaces fail CI immediately.

## How to Apply

When `bin/flow plan-check` returns a violation tagged
`external-input-audit`:

1. Read the cited line in the plan file. Identify the function
   the proposal tightens.
2. Grep for the function's callers across `src/` (the rule file
   `.claude/rules/external-input-validation.md` lists the
   canonical hook callsite inventory as one example —
   `stop_continue.rs`, `stop_failure.rs`, `post_compact.rs`,
   `validate_ask_user.rs`, `validate_claude_paths.rs`).
3. Build the four-column audit table near the trigger in the
   plan. For each caller row:
   - **Caller** — file path and line range
   - **Source** — where the value enters this caller (state-file
     key, git subprocess output, CLI flag, etc.)
   - **Classification** — `Guaranteed valid`,
     `Trusted-but-external`, or `Untrusted`
   - **Handling** — which constructor variant the caller will
     use, plus the control-flow on invalid input
4. Re-run `bin/flow plan-check`. If clean, proceed to phase
   completion.

If the trigger is a discussion mention rather than a proposal, add
the `<!-- external-input-audit: not-a-tightening -->` opt-out
comment using the walk-back rule above.

## Cross-References

- `.claude/rules/external-input-validation.md` — the prose
  discipline (constructor patterns, `try_new` convention, hook
  callsite rules).
- `.claude/rules/scope-enumeration.md` — the structurally sibling
  gate this design is modeled on, sharing the windowing
  heuristic, opt-out grammar, and three-callsite topology.
- `src/external_input_audit.rs` — the scanner implementation.
- `src/plan_check.rs` — the standard-path gate.
- `src/plan_extract.rs` — the extracted and resume gates.
- `tests/external_input_audit.rs` — the corpus contract test.
