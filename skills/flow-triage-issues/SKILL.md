---
name: flow-triage-issues
description: "Triage a single open GitHub issue from a PM lens. Reads code, checks for already-shipped work, returns a verdict in {close, decompose, keep-open, fix-now} with confidence and a flip-condition. Renders and stops — no side effects."
---

# FLOW Triage Issues

Run a structured per-issue triage from a PM-with-engineering-literacy
lens. Dispatches the `issue-triage` sub-agent in the foreground, which
fetches the issue, reads referenced code (or grep-locates behavior when
unreferenced), checks for already-shipped work via
`gh pr list --search` and `git log --all --grep`, and answers 10
triage questions plus a verdict card. The skill renders the verdict
verbatim and STOPS — the PM acts manually.

## Usage

```text
/flow:flow-triage-issues <issue-number>
```

The argument is a positive integer issue number in the current
repository (whichever repo `gh` resolves to). v1 is open issues
only — closed issues are refused with an out-of-scope envelope.

## Concurrency

This skill is read-only with respect to GitHub state. It never
closes, labels, comments on, or otherwise mutates issues.
Concurrent invocations from different sessions cannot collide on
shared state. The sub-agent's `gh issue view` and `gh pr list`
calls are read-only; multiple parallel triages on different issues
are safe.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-triage-issues — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Parse argument

Read the argument string. Strip surrounding whitespace and a single
leading `#` if present.

The argument MUST match the regex `^[1-9][0-9]*$` exactly — a
positive decimal integer with no leading zero, no sign, no decimal
point, no scientific notation, no whitespace, no quotes, no flags.
The strict shape rejects argument-injection vectors like
`42 --repo other/repo`, regex-metacharacter values like `1[23]`,
floats like `1.5`, and zero/negative values that the GitHub API
treats as flags.

- If empty (no argument): use AskUserQuestion to ask
  "Which issue number should I triage?" with no preset options. Use
  the user's reply as the issue number, then re-validate against
  the regex above.
- If the argument does NOT match the regex: output the following
  error in your response (not via Bash) inside a fenced code block,
  then stop:

````markdown
```text
Error: /flow:flow-triage-issues requires a positive integer issue number.
Got: <argument>
Usage: /flow:flow-triage-issues <issue-number>
```
````

- If the argument matches: keep the value as `<ISSUE_NUMBER>` for
  Step 2.

### Step 2 — Dispatch the issue-triage sub-agent

Invoke the `issue-triage` sub-agent in the foreground via the Agent
tool. Pass `<ISSUE_NUMBER>` as the labeled `ISSUE_NUMBER` input.

Wait for the sub-agent to return its full output. The sub-agent does
all the investigation — gh fetches, code reads, shipped-work checks,
question answers, verdict construction. The skill performs no `gh`
or `git` calls itself.

### Step 3 — Check for the structural marker

Before rendering, scan the agent's returned output for the literal
`## END-OF-FINDINGS` completion marker (per
`.claude/rules/cognitive-isolation.md` "Context Budget +
Truncation Recovery"). Marker absence means the agent ran out of
turns mid-investigation and the partial output is unsafe to render.

When the marker IS present, additionally verify the agent produced
either a complete verdict card or an out-of-scope envelope:

- A complete verdict card requires a `### Verdict` heading
  followed by ALL FIVE labels appearing somewhere after the
  heading: `Disposition`, `Summary`, `Evidence`, `Confidence`,
  `This flips if`. A response with `### Verdict` but missing any
  of the five labels is an echo of the agent's own template, not
  a real verdict — treat as truncated.
- An out-of-scope envelope requires a `### Out of scope` heading
  followed by `Reason`, `Detail`, and `Next step for the PM`
  labels. Same shape.

Decision tree:

- If `## END-OF-FINDINGS` is present AND a complete verdict card
  OR a complete out-of-scope envelope is present → proceed to
  Step 4.
- Otherwise → output the following message in your response (not
  via Bash) inside a fenced code block, then stop without
  rendering the partial output:

````markdown
```text
Investigation incomplete: the issue-triage sub-agent did not produce
a complete verdict card or out-of-scope envelope followed by the
`## END-OF-FINDINGS` marker. The agent likely ran out of turns
mid-investigation. Try invoking the skill again, or open the issue
manually and triage it yourself.
```
````

### Step 4 — Render the verdict verbatim

Print the agent's complete output inline in your response — every
heading, every bullet, every citation. Do not summarize, paraphrase,
re-rank, or trim. The verdict format (5 fields: disposition, summary,
evidence, confidence, flip-condition) and the 4-disposition closed
set (`close`, `decompose`, `keep-open`, `fix-now`) are locked by
contract tests. The PM consuming the verdict must see exactly what
the agent produced.

### Step 5 — STOP

<HARD-GATE>
After rendering the verdict, stop. Do NOT take any auto-action based
on the disposition — no auto-close, no auto-label, no auto-comment,
no auto-invocation of follow-on skills.

This HARD-GATE is mechanical. You must NOT:

- Invoke any skill via the Skill tool after rendering the verdict
  (regardless of what the disposition value is)
- Run `gh issue close`, `gh issue edit`, `gh issue comment`, or any
  other GitHub-state-mutating subcommand
- Run any `git` command that writes (commit, push, tag, etc.)
- Take any action whatsoever based on the disposition value

The PM reads the verdict and decides what to do. Print a brief
hint describing the next manual step based on the disposition,
inside a fenced code block. Describe the action in prose — do
NOT include slash-command literals that the model could be
tempted to invoke. The PM types the next command themselves.

- **close** — describe the manual step as: "Read the evidence to
  confirm, then close the issue manually via the GitHub UI or your
  CLI of choice."
- **decompose** — describe the manual step as: "The issue needs an
  Implementation Plan; draft a pre-decomposed replacement
  yourself, then close the original."
- **keep-open** — describe the manual step as: "Leave the issue
  open and revisit later — no action needed now."
- **fix-now** — describe the manual step as: "Start a new flow
  against the issue yourself when you are ready to work on it."
- **Out of scope** (closed issue or fetch failure) — describe the
  manual step as: "Open the issue in a browser and triage
  manually."

Then output the COMPLETE banner and stop. Do not run any other tool
or invoke any other skill.
</HARD-GATE>

## Done

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-triage-issues — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never close issues. The skill is read-only with respect to GitHub
  state — no `gh issue close`, no `gh issue edit`, no
  `gh issue comment`, no labels.
- Never auto-invoke `/flow:flow-create-issue`, `/flow:flow-start`, or
  any other skill based on the verdict. The PM acts manually.
- v1: open issues only. The agent refuses closed issues with the
  out-of-scope envelope; the skill renders that envelope cleanly.
- Verdict format is exactly the 5-field card produced by
  `agents/issue-triage.md`. Do not paraphrase, re-rank, summarize, or
  trim the agent's output.
- Disposition values are exactly `{close, decompose, keep-open,
  fix-now}`. The closed set is locked by contract test; never
  introduce additional values — the agent never produces them.
- Use the `issue-triage` sub-agent only. Other agents are out of
  scope for this skill (the contract test enforces this).
- Render and stop. No auto-actions of any kind.
