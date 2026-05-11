# Include Bias in Issues

**Default to inclusion when filing or scoping an issue.** The
question is not "should this be included?" but "is there a
*concrete* reason this must NOT be included?" Absent a concrete
reason, the work belongs in scope.

## Lifecycle Cost

Every concern split out of an issue spawns a full Plan → Code →
Review → Learn → Complete cycle on the same files the
original flow already explored. Splitting compounds: the second
flow re-reads the same code, re-derives the same context, and
runs the same gates. Including is bounded — one extra task in
the same plan against the same exploration.

The math:

- **Including** an adjacent concern: O(1) — one extra task in
  the current Tasks section. The Plan-phase exploration budget
  already covers the files. Code-phase TDD adds one cycle.
- **Splitting** an adjacent concern: O(N) — a new issue, a new
  flow lifecycle, redundant exploration of the same files,
  another Review pass, another Learn audit, another merge.

The lifecycle cost is the persuasive hook. When the cost of
inclusion fits in the current flow's exploration budget, the
cost of splitting is multiples larger.

## Bad Reasoning Patterns

These framings invite preemptive scope shrinkage. Each looks
like a valid exclusion criterion but is not — none of them
identify a concrete blocker.

| Pattern | Why it's not a valid exclusion |
|---|---|
| "the prior PR did not touch this" | Prior PRs scoped to their own moment. The current PR is a new judgment call against current state. Prior boundaries are not load-bearing. |
| "user owns this" | Code ownership is not exclusion criteria. Every line of code has an owner; that owner is not present in the conversation, and the deferral becomes "wait for the owner" — a new lifecycle cost. |
| "separate code surface" | Concerns that touch a different file or module are still concerns the current PR can address if the exploration already covered them. "Separate surface" describes the code, not the work. |
| "would expand scope" (reflexive) | The reflex to exclude work because including grows the diff is not exclusion criteria. The valid question is whether the addition fits the current exploration budget. `.claude/rules/scope-expansion.md` is the systematic decision tree for sweeping fixes — three conditions (inert fixes, single guard, splitting would re-do work) gate when scope expansion is the right call. The bad pattern is reflexive "no, that's expansion"; the good pattern is "let me apply the three-condition gate." |
| "Out of Scope as defensive enumeration" | A list of exclusions written before the work begins is speculation, not analysis. Real exclusions emerge from concrete blockers discovered during exploration, not from prudence. |

When you reach for one of these patterns, the move is to convert
the deferral into an inclusion task — or to name the concrete
blocker that makes inclusion impossible.

The "would expand scope" row deserves an extra note: this rule
does NOT dismiss `.claude/rules/scope-expansion.md`. The two
rules cover complementary surfaces. `scope-expansion.md` is the
Plan-phase decision tree for sweeping fixes — it tells the
author when expanding scope IS the right call (inert fixes +
single guard + splitting would re-do work) and when bounding to
the cited issue is right (any condition fails). This rule
forbids using "would expand scope" as a reflexive shortcut to
skip the three-condition analysis. Apply the gate, then decide.

## Narrow Valid Exclusions

Genuine exclusion is rare. The short list of valid criteria:

- **The user explicitly rejected the scope in conversation.**
  When the user has named what they want excluded, honor the
  directive. Their rejection is the criterion.
- **Including would require a different design conversation.**
  When the addition opens architectural questions the current
  exploration cannot answer (e.g., "this concern needs a new
  module", "this requires a security review"), excluding is
  correct because the work is not just larger but qualitatively
  different.
- **Including would block the issue's primary completion
  criteria.** When the addition introduces dependencies the
  primary work cannot ship without, exclude — but file the
  follow-up at the same time and link it via
  `bin/flow link-blocked-by` so the dependency is visible.

When an exclusion is genuinely warranted, write prose in the
Context or Problem section explaining the boundary — not a
templated section that invites enumeration. A one-sentence
rationale grounded in a concrete blocker is the right shape;
a bulleted list of "things we are not doing" is not.

## Canonical Scan Phrasings

The mechanical backstop scans for these four canonical
phrasings. Future changes to the scanned set must update both
this enumeration and the corresponding subsections in
`skills/flow-create-issue/SKILL.md` and
`skills/flow-decompose-project/SKILL.md` so the rule remains
the authoritative source for what the scans target. The SKILL
scan instructions read case-flexibly in practice — the model
interprets each phrasing as a concept and catches common
section-heading title-case variants in issue bodies (e.g.
`## Out of Scope`) alongside the canonical lowercase forms:

- `"Out of scope"` — defensive enumeration of exclusions
  written before concrete blockers have surfaced
- `"Non-goals"` — same defensive-enumeration shape under a
  different heading; a bulleted list of "things we are not
  doing" is speculation, not analysis
- `"would expand scope"` — reflexive scope shrinkage that
  bypasses the three-condition gate in
  `.claude/rules/scope-expansion.md`
- `"separate code surface"` — code-shape framing used as an
  exclusion criterion; "separate surface" describes the code,
  not the work

## How to Apply

### Plan Phase

When drafting the Implementation Plan's Exploration and Tasks
sections, include adjacent concerns by default. For every
candidate concern that surfaces during exploration:

1. **Default to a task.** Add the concern as a task in the Tasks
   section unless one of the narrow valid exclusions applies.
2. **Apply the lifecycle-cost test.** If excluding would force
   a future flow to re-explore the same files, include now.
3. **Apply the scope-expansion gate when sweeping.** If the
   exploration uncovered a class of concerns wider than the
   issue cited, run the three-condition gate from
   `.claude/rules/scope-expansion.md` (inert fixes + single
   guard + splitting would re-do work) before deciding to
   bound. Bounding is correct when any condition fails;
   "would expand scope" alone is not.
4. **Name the blocker if excluding.** Write the rationale in
   the Context section as one sentence naming the concrete
   blocker. No templated list.

### Code Phase

When discovery during implementation reveals an adjacent
concern that the plan did not name, include it in the same PR
when the lifecycle-cost test favors inclusion. Per
`.claude/rules/scope-expansion.md`, expansion is the right call
when the fixes are inert, a single guard prevents regression,
and splitting would re-do work.

When a Code-phase discovery genuinely requires a different
design conversation, log the deferral via `bin/flow log` per
`.claude/rules/plan-commit-atomicity.md` "Plan Signature
Deviations Must Be Logged" and file a follow-up.

### Review Phase

When triaging findings, apply the supersession test from
`.claude/rules/supersession.md` first — code the PR has made
redundant gets deleted regardless of file location. Then apply
the inclusion-bias question to every Real finding: "would
fixing this in the current PR cost less than filing a follow-up
flow?" Per `.claude/rules/review-scope.md`, the answer is
almost always yes; filing a follow-up for a real finding is
effort optimization dressed up as scope discipline.

## Cross-References

- `.claude/rules/scope-expansion.md` — the Plan-phase decision
  tree for sweeping fixes. Names the three conditions under
  which expanding scope to cover the full sweep is correct
  (inert fixes, single guard, splitting would re-do work).
  Complementary, not conflicting: this rule forbids reflexive
  "would expand scope" exclusion; the scope-expansion rule
  provides the gate that decides whether expansion is correct.
- `.claude/rules/filing-issues.md` — the broader filing
  discipline. Include-bias is the upstream principle; filing
  rules are the downstream mechanics (cold-start writing,
  evidence verification, repo routing).
- `.claude/rules/review-scope.md` — Review's
  every-real-finding-fixed-in-PR rule. The same lifecycle-cost
  framing motivates both: filing what you can fix is more
  expensive than fixing it.
