---
title: /flow-triage-issue
nav_order: 16
parent: Skills
---

# /flow-triage-issue

**Phase:** Any

**Usage:**

```text
/flow-triage-issue <issue-number>
```

Triage a single open GitHub issue from a senior-PM-with-engineering-literacy
lens. Dispatches the `issue-triage` sub-agent in the foreground. The agent
fetches the issue, reads referenced code (or grep-locates behavior when
the issue body names no files), checks `gh pr list --search "<num>"` and
`git log --all --grep "#<num>"` for already-shipped work, answers ten
triage questions, and produces a verdict in `{close, decompose}` with
confidence and a flip-condition. The skill renders the verdict
verbatim and stops — no auto-actions.

---

## What It Does

1. Parses the argument as a positive integer issue number; rejects
   non-numeric input and prompts when the argument is missing.
2. Dispatches the `issue-triage` sub-agent (model: `sonnet`,
   read-only tools, no `Edit`/`Write`).
3. Checks the agent's output for a `### Verdict` or `### Out of scope`
   structural marker. If neither is present, the agent ran out of turns
   mid-investigation; the skill reports "investigation incomplete" and
   stops without rendering the partial output.
4. Renders the agent's full output inline — every heading, bullet, and
   `file:line` citation, exactly as the agent produced it.
5. Prints a one-line hint pointing at the next manual step based on the
   disposition (e.g. `gh issue close <num>` for `close`,
   `/flow:flow-create-issue` for `decompose`).

---

## The 10-Question Lens

The agent answers ten questions in plain English, citing `file:line` for
every code claim:

1. Real? (evidence-grounded)
2. Still real? (current code state)
3. Framing — actual problem or symptom?
4. What (plain English)
5. Why care (plain English)
6. Who's affected and how severely?
7. How urgent?
8. How would this be fixed?
9. What does success look like?
10. Risk of the fix.

---

## The 2-Disposition Verdict

| Disposition | Meaning | Next manual step |
|---|---|---|
| `close` | No longer a real problem (already shipped, framing wrong, behavior changed) | `gh issue close <num>` after reading evidence |
| `decompose` | Real and ready for implementation planning; needs an Implementation Plan before `/flow:flow-start` | `/flow:flow-create-issue` to draft a pre-decomposed replacement |

The set is closed in v1. Adding new dispositions requires a separate
design conversation.

---

## What This Skill Does NOT Do

- **Never closes issues.** No `gh issue close`. The PM closes manually
  after reading the evidence.
- **Never adds labels.** No `gh issue edit --add-label`.
- **Never comments.** No `gh issue comment`.
- **Never auto-invokes follow-on skills.** Render the verdict, stop,
  print the next-step hint. The PM types the next command.
- **Never triages closed issues.** v1 refuses closed issues with an
  out-of-scope envelope.
- **Never triages PRs.** PR review is `/code-review:code-review`'s
  domain.

---

## Gates

- Read-only on GitHub state — no mutations
- Display-only after the agent returns — no auto-actions
- The `### Verdict` / `### Out of scope` structural marker is required;
  partial output (sub-agent truncation) is not rendered as if complete
