# Semi-Formal Reasoning

When adding or modifying a FLOW agent, evaluate whether it should
include a Reasoning Discipline section with the Premise → Trace →
Conclude template.

## When to Include

Include the reasoning discipline when the agent:

- Produces prose findings about code behavior (bugs, risks,
  violations, coverage gaps)
- Reasons about execution paths, data flow, or state transitions
- Makes claims that depend on how code actually behaves at runtime

Current agents with the discipline: pre-mortem, reviewer, ci-fixer
(deep-diagnosis mode only), adversarial.

## When Not to Include

Skip the reasoning discipline when the agent:

- Produces concrete artifacts rather than prose findings (e.g.,
  a code generator that outputs compilable code)
- Performs process analysis rather than code-semantic analysis
  (e.g., learn-analyst reviewing workflow friction)
- Evaluates comprehension or documentation rather than behavior
  (e.g., onboarding agent reviewing readability)

Current agents without the discipline: learn-analyst, onboarding.

## The Template

Each finding follows three steps:

- **Premise** — state the claim and cite specific file paths and
  line ranges
- **Trace** — walk the execution path step by step, verifying each
  step with Read or Grep
- **Conclude** — confirm or refute the premise based on the trace

Findings with incomplete traces must be discarded, not reported
with caveats.

## Speed-Sensitive Agents

When an agent has a speed-critical first pass (like ci-fixer),
scope the discipline to retry attempts only. The first attempt
uses direct diagnosis. Semi-formal reasoning activates on attempt
2+ when the fast path has failed and deeper analysis is needed.
