# Autonomous-Flow Self-Recovery

When a tool call fails during an autonomous phase (`continue: auto`),
classify the failure as **mechanical** or **substantive** before
deciding whether to ask the user. Mechanical failures are
deterministic and the error message names the fix; the model resolves
them in-flow. Substantive failures require domain judgment the model
cannot supply unilaterally; the model surfaces them via
`AskUserQuestion` subject to `.claude/rules/autonomous-phase-discipline.md`.

Without this classification, the model defaults to "ask the user"
on every failure, defeating the autonomous-mode contract — every
unnecessary prompt costs trust and round-trip latency. The point
of `continue: auto` is that the model already has authorization to
work through the deterministic friction; pausing for confirmation
is exactly the interruption the configuration is meant to prevent.

## The Rule

When a tool call fails during an autonomous phase, ask:

> **Does the failure message name the fix, or does it ask a question
> the model is not authorized to answer?**

- If the message names the fix (mechanical) → resolve in-flow, log
  the rerouting via `bin/flow log` for the Learn-phase analyst, and
  retry the action with the corrected input.
- If the failure depends on domain judgment, semantic intent, or
  evidence the model lacks (substantive) → surface via
  `AskUserQuestion`. The autonomous-phase block does not apply when
  the question is genuinely substantive — the discipline forbids
  *unnecessary* prompts, not all prompts.

## Mechanical Failures (Resolve In-Flow)

These failures are deterministic. The error message either names the
correct destination, names the correct tool, or names a structurally
identical retry. Apply the fix and retry — do not prompt the user.

- **`validate-worktree-paths` hook rejections** — the hook responds
  with a `BLOCKED:` message that names the canonical destination.
  Reissue the call against the canonical path. (Reference:
  `.claude/rules/file-tool-preflights.md` "Canonical Location for
  `.flow-states/`".)
- **Path mismatches between worktree and main repo** — the hook
  redirects worktree-internal `.flow-states/` writes to the main
  repo's canonical location. Reissue against the main repo path.
- **Relative-vs-absolute path confusion** — when a CLI subcommand or
  file tool rejects a relative path, retry with the absolute form
  (or vice versa) using the path the error message names.
- **Read-tool failures on just-written paths** — when the file was
  written via `bin/flow write-rule` rather than the Write tool, the
  Read-tracking is not in this session's tool history. Reissue the
  Read once; the file is now on disk and the read succeeds.
- **Compound-command rejections from `validate-pretool`** — the hook
  rejects `&&`, `;`, `|`, command substitution, and shell redirection
  (`>`, `<`) outside quoted arguments. Split into separate Bash tool
  calls or use the Read/Grep/Glob tool the message recommends.
- **File-read rejections under `validate-pretool` Layer 6** — `git
  diff` with file-path arguments is blocked; retry via the Read tool
  for content or the Grep tool for pattern matches as the error
  message instructs.

## Substantive Failures (Prompt the User)

These failures depend on intent, business meaning, or evidence the
model cannot reconstruct. The autonomous-phase-discipline block
does not apply — `validate-ask-user` allows the prompt through when
the situation is genuinely substantive.

- **Domain ambiguity** — the plan task names two valid
  interpretations and the surrounding prose does not disambiguate
  (e.g. "deduplicate the entries" when "deduplicate" could mean
  exact-string-match or semantic-equivalence).
- **Semantic decisions** — design choices that affect user-visible
  behavior (which error message wording to use; which of two valid
  algorithms to pick; whether to bump a major or minor version).
- **User-evidence contradictions** — when the user's screenshot,
  log line, or stated observation disagrees with the model's code
  reading. The user's evidence is ground truth; surface the
  contradiction so the user can correct the model's hypothesis.

## How to Apply

**During Code phase.** When a tool call returns a `BLOCKED:` or
similar error, read the message. If it names the canonical
destination, the correct tool, or a structurally identical retry,
classify as mechanical and rerun. Log the rerouting via `bin/flow
log <branch> "[Phase 3] Mechanical recovery: <what failed> →
<retry shape>"` so the Learn-phase audit can confirm the
discipline.

**During Code Review phase.** When a sub-agent's output shows it
hit a mechanical failure and recovered, that is the rule working —
not a finding. When the agent prompted the user for a mechanical
failure, surface it as a process gap: the gap is in the agent's
prompt, not in the agent's reasoning.

**During Learn phase.** The learn-analyst audits the session log
for mechanical-recovery entries and substantive prompts. A
substantive prompt without a clear domain question is a gap (the
model should have classified mechanical). A mechanical recovery
that took more than one retry is a gap (the rerouting itself
needs work). Both surface as findings the analyst routes to a
rule update or skill clarification.

## Cross-References

- `.claude/rules/autonomous-phase-discipline.md` — the parent rule
  forbidding unnecessary prompts during autonomous phases. This
  rule (self-recovery) tells the model how to satisfy that
  discipline when the failure is mechanical.
- `.claude/rules/fix-infrastructure-bugs.md` — when the failure
  is a real infrastructure bug (lock file corruption, hook misfire
  with no documented fix), pivot to fixing the bug rather than
  retrying. The mechanical-recovery path is for failures with
  documented fixes; an undocumented infrastructure bug is the
  fix itself.
- `.claude/rules/anti-patterns.md` "Never Offer to Skip Workflow
  Steps" — even when a failure looks mechanical, never skip the
  step it blocked. Recovery means rerouting around the friction,
  not removing the gate.
- `.claude/rules/forward-facing-authoring.md` — the prose
  discipline this rule itself follows. The rule describes the
  classification and the response shape, not the specific
  incidents that motivated it.
