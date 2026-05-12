---
name: flow-triage-issue
description: "Triage a single open GitHub issue from a PM lens. Applies a 'Triage In-Progress' label during triage; reads code, checks for already-shipped work, returns a verdict in {close, decompose} with confidence and a flip-condition. Renders and stops — no other side effects."
---

# FLOW Triage Issue

Run a structured per-issue triage from a PM-with-engineering-literacy
lens. Dispatches the `issue-triage` sub-agent in the foreground, which
fetches the issue, reads referenced code (or grep-locates behavior when
unreferenced), checks for already-shipped work via
`gh pr list --search` and `git log --all --grep`, and answers 10
triage questions plus a verdict card. The skill renders the verdict
verbatim and STOPS — the PM acts manually.

## Usage

```text
/flow:flow-triage-issue #1234
```

The argument is `#N` — a literal `#` followed by a positive integer
issue number in the current repository (whichever repo `gh`
resolves to). v1 is open issues only — closed issues are refused
with an out-of-scope envelope.

## Concurrency

This skill mutates two surfaces of GitHub state. **Per triage**, it
applies a "Triage In-Progress" label assignment to the issue at the
start of triage and removes it before the COMPLETE banner.
**One-time per repository**, Step 2 also creates the
"Triage In-Progress" label in the repository's label registry on
the first triage in a fresh repo — the registry write is idempotent
in intent (the steady-state path checks for existence first and
skips the create). The skill never closes, comments on, or applies
any other label.

The per-issue label assignment is a passive observability signal —
a second PM running `/flow:flow-triage-issue` on an issue already
labeled sees the in-progress signal in the GitHub UI before
invoking; it is NOT a mutex, so concurrent triages of the same
issue are tolerated and whichever invocation removes the label
last wins. If the skill crashes between the add and remove, the
label persists; manual removal from the GitHub UI is the recovery
path. The sub-agent's `gh issue view` and `gh pr list` calls
remain read-only; the agent itself never mutates state. Multiple
parallel triages on different issues are safe.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-triage-issue — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Parse argument

Read the argument string. Strip surrounding whitespace.

The argument MUST match the regex `^#[1-9][0-9]*$` exactly — a
literal `#` followed by a positive decimal integer with no leading
zero, no sign, no decimal point, no scientific notation, no
whitespace, no quotes, no flags. The strict `#` prefix matches the
sibling `/flow:flow-start` and `/flow:flow-plan` argument
formats so issue references are unambiguous across the FLOW skill
family. The strict shape rejects argument-injection vectors like
`#42 --repo other/repo`, regex-metacharacter values like `#1[23]`,
floats like `#1.5`, and zero/negative values that the GitHub API
treats as flags.

- If empty (no argument): use AskUserQuestion to ask
  "Which issue number should I triage?" with no preset options. Use
  the user's reply as the issue number (prepending `#` if the user
  omitted it), then re-validate against the regex above.
- If the argument does NOT match the regex: output the following
  error in your response (not via Bash) inside a fenced code block,
  then stop:

````markdown
```text
Error: /flow:flow-triage-issue requires `#N` where N is a positive integer.
Got: <argument>
Usage: /flow:flow-triage-issue #<issue-number>
```
````

- If the argument matches: strip the leading `#` and keep the
  numeric value as `<issue_number>` for the remaining steps.

### Step 2 — Apply the Triage In-Progress label

Apply a "Triage In-Progress" label to the issue so concurrent
sessions can see at a glance that the issue is being triaged. The
label is passive — it does not block other invocations — but it
makes parallel work visible in the GitHub UI.

First, check whether the label already exists in the repository.
`gh issue edit --add-label` rejects unknown labels, so a fresh
repo needs the label created before the per-issue assignment can
land:

```bash
gh label list --search "Triage In-Progress" --json name
```

Parse the JSON output. The result is a JSON array — empty `[]`
means the label is absent and must be created; any non-empty array
containing an entry whose `name` field equals "Triage In-Progress"
means the label already exists and the create call must be
skipped.

If the list-search is empty (no existing label), create it:

```bash
gh label create "Triage In-Progress" --description "PM is currently triaging this issue" --color FFA500
```

If `gh label create` fails for any reason in this branch (network,
auth, permission), halt the skill and surface the error to the
user — do NOT proceed to add-label, because the assignment will
also fail.

If the list-search returned a non-empty array containing
"Triage In-Progress", skip the create call entirely and proceed
to the assignment.

Add the label to the issue:

```bash
gh issue edit <issue_number> --add-label "Triage In-Progress"
```

`<issue_number>` is the validated numeric value from Step 1.

### Step 3 — Dispatch the issue-triage sub-agent

Invoke the `issue-triage` sub-agent in the foreground via the Agent
tool. Pass `<issue_number>` as the labeled `ISSUE_NUMBER` input.

Wait for the sub-agent to return its full output. The sub-agent does
all the investigation — gh fetches, code reads, shipped-work checks,
question answers, verdict construction. The skill performs no `gh`
or `git` calls itself beyond the label add/remove around this
dispatch.

### Step 4 — Check for the structural marker

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
  Step 5.
- Otherwise → remove the "Triage In-Progress" label first (the
  label-remove must run on every exit path so the issue does not
  carry a residual label after a truncated investigation):

```bash
gh issue edit <issue_number> --remove-label "Triage In-Progress"
```

  Then output the following message in your response (not via
  Bash) inside a fenced code block, then stop without rendering
  the partial output:

````markdown
```text
Investigation incomplete: the issue-triage sub-agent did not produce
a complete verdict card or out-of-scope envelope followed by the
`## END-OF-FINDINGS` marker. The agent likely ran out of turns
mid-investigation. Try invoking the skill again, or open the issue
manually and triage it yourself.
```
````

### Step 5 — Render the verdict verbatim

Print the agent's complete output inline in your response — every
heading, every bullet, every citation. Do not summarize, paraphrase,
re-rank, or trim. The verdict format (5 fields: disposition, summary,
evidence, confidence, flip-condition) and the 2-disposition closed
set (`close`, `decompose`) are locked by contract tests. The PM
consuming the verdict must see exactly what the agent produced.

### Step 6 — Remove the Triage In-Progress label

The verdict has been rendered (or the out-of-scope envelope was
displayed). Remove the "Triage In-Progress" label so the issue no
longer signals active triage. The Step 7 HARD-GATE that follows
forbids any further `gh issue edit` calls — the label cleanup must
land in this step before the gate fires.

```bash
gh issue edit <issue_number> --remove-label "Triage In-Progress"
```

### Step 7 — STOP

<HARD-GATE>
After Step 6's label-removal completes, stop. Do NOT take any
auto-action based on the disposition — no auto-close, no
auto-label, no auto-comment, no auto-invocation of follow-on
skills. The Step 6 label-remove is the LAST mutation this skill
performs; everything below this gate is post-cleanup.

This HARD-GATE is mechanical. After Step 6 completes you must NOT:

- Invoke any skill via the Skill tool after rendering the verdict
  (regardless of what the disposition value is)
- Run any further `gh issue close`, `gh issue edit`,
  `gh issue comment`, or any other GitHub-state-mutating
  subcommand (Step 6's `gh issue edit ... --remove-label` was the
  final permitted invocation; no further `gh issue edit` calls are
  permitted from this point)
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
  ✓ FLOW v1.1.0 — flow:flow-triage-issue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never close issues. The skill never runs `gh issue close`.
- Never comment on issues. The skill never runs `gh issue comment`.
- Never apply or remove labels other than the "Triage In-Progress"
  marker the skill manages itself: applied in Step 2, removed in
  Step 6 on the happy path, and also removed in Step 4's
  truncation early-stop branch so every exit path leaves the
  issue free of the label.
- Never auto-invoke `/flow:flow-explore`, `/flow:flow-plan`,
  `/flow:flow-start`, or any other skill based on the verdict. The
  PM acts manually.
- v1: open issues only. The agent refuses closed issues with the
  out-of-scope envelope; the skill renders that envelope cleanly.
- Verdict format is exactly the 5-field card produced by
  `agents/issue-triage.md`. Do not paraphrase, re-rank, summarize, or
  trim the agent's output.
- Disposition values are exactly `{close, decompose}`. The closed
  set is locked by contract test; never introduce additional
  values — the agent never produces them.
- Use the `issue-triage` sub-agent only. Other agents are out of
  scope for this skill (the contract test enforces this).
- Render and stop. No auto-actions beyond the Triage In-Progress
  label add/remove.
