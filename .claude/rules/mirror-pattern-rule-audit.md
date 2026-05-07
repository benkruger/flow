# Mirror-Pattern Rule Audit

When a plan's Exploration or Approach section says the new code will
"mirror sibling X exactly" — copying the field reads, gate
comparisons, fail-open posture, or any other pattern from an existing
function — the plan must also enumerate whether sibling X currently
complies with every applicable rule. A literal mirror inherits any
pre-existing rule violations as new code, and Code Review will flag
them in the new file even though they were copied verbatim from an
older one.

## Why

"Mirror sibling X exactly" is a shortcut for "the existing pattern
works, do that." It saves Plan-phase exploration time and produces
internally consistent code. But the shortcut has a hidden cost: it
copies whatever the sibling does, including any latent rule
violations the sibling has accumulated while rules tightened around
it.

Two failure modes recur:

1. **Inherited violation.** Sibling X uses raw `==` comparisons on
   state-derived strings; `security-gates.md` "Normalize Before
   Comparing" was added after X was written. The new function
   mirrors X, so it inherits the un-normalized comparison. Code
   Review flags it. The fix lands in the new function only,
   leaving X out of date.
2. **Drift on rule update.** A future rule change tightens
   `security-gates.md`. The author updates X. The mirror in the
   new function is forgotten because nothing tied them together.
   The two predicates silently diverge.

The Plan-phase audit catches both: by listing the sibling's
applicable rules at planning time, the author either fixes both
together (extracting a shared helper, planning a second commit) or
explicitly accepts the inheritance (logging the gap and filing tech
debt for the sibling).

## The Rule

When a plan task description, Exploration entry, or Approach prose
contains any of these mirror-pattern phrasings:

- "mirror sibling X exactly"
- "matches sibling X's pattern"
- "follow X's convention"
- "use the same pattern as X"
- "copy from X"
- "parallels the implementation of X"

…the plan must include a **Mirror Audit Table** within a few lines
of the trigger. The table has three columns:

| Sibling pattern | Applicable rules | Compliance |
|---|---|---|
| `validate_ask_user::validate` — reads `phases.<phase>.status`, `skills.<phase>` and compares to literal strings | `security-gates.md` "Normalize Before Comparing"; `external-input-path-construction.md` "Enforce a documented size cap" | UN-NORMALIZED — `== Some("auto")` raw byte equality (gap). NO BYTE CAP — uses `mutate_state` which has internal bounds. |
| `check_first_stop` — fail-open on parse error | `security-gates.md` "Fail Closed When State Is Unreliable" | TENSION — module doc commits to fail-open across the hook family for user-recovery; rule's fail-closed posture would deadlock under corrupt state. Documented exception. |

Column definitions:

- **Sibling pattern** — file:function and the specific behavior the
  new code will mirror.
- **Applicable rules** — every `.claude/rules/*.md` file whose
  trigger conditions match this pattern. Cite the rule's relevant
  section heading.
- **Compliance** — one of:
  - *Compliant* — sibling X follows the rule. Mirror inherits the
    compliant pattern.
  - *Gap* — sibling X violates the rule. Mirroring inherits the
    violation. The plan must decide: extract a shared helper that
    fixes both, OR mirror the violation in the new code AND file
    a Tech Debt issue for the sibling, AND add the new code's fix
    to the same PR.
  - *Tension* — sibling X follows a pattern that contradicts the
    rule, but for a documented reason. The plan must reference the
    documenting source (module doc, prior rule clarification) so
    the new code can adopt the same exception.

## How to Find Applicable Rules

For each pattern the plan is about to mirror, run a targeted grep
of `.claude/rules/` for keywords describing the pattern:

```text
grep -l "Normalize Before Comparing" .claude/rules/
grep -l "BYTE_CAP\|byte cap\|Enforce a documented size cap" .claude/rules/
grep -l "fail-open\|Fail Closed" .claude/rules/
grep -l "panic.*assert\|invariant check" .claude/rules/
```

Every matching rule is a candidate; read each match and decide if
the rule's trigger conditions match the pattern being mirrored.

## How to Apply

**Plan phase.** When writing Exploration prose, watch for
mirror-pattern phrasings. For each one, build the audit table.
Include the table in the plan's Exploration or Approach section
before Code phase begins.

**Code phase.** When implementing a mirror, follow the audit
table's compliance column:

- **Compliant** rows: copy the pattern verbatim.
- **Gap** rows: do not copy the violation. Implement the new code
  with the rule-compliant pattern. File the Tech Debt issue for
  the sibling.
- **Tension** rows: copy the documented exception. Reference the
  documenting source in a code comment so future readers see the
  rule tension explicitly.

**Code Review phase.** The reviewer agent cross-checks the audit
table against the landed code. A finding tagged "mirror inherited
a known violation" routed to Step 4 either fixes the new code or
records a Plan-phase gap (the audit was missing or incomplete).

## Plan-Phase Trigger

This rule is enforcement-light: no mechanical scanner currently
catches missing audit tables, because mirror-pattern phrasings
are too common in legitimate prose ("mirror this in tests" is
not the same as "mirror sibling X exactly"). The Plan-phase
exploration is where the audit happens; the Code Review reviewer
agent is the safety net.

## Cross-References

- `.claude/rules/security-gates.md` — the canonical gate-input
  rules that mirror audits most often surface.
- `.claude/rules/external-input-path-construction.md` — the
  byte-cap rule for filesystem reads.
- `.claude/rules/external-input-validation.md` — the parent
  prose discipline for fallible constructors.
