# Transcript Shape

Claude Code's persisted transcript JSONL contains both user-typed
turns and synthetic system-generated turns under the same
`type:"user"` discriminator. Walkers that need to find the most
recent REAL user turn must call
`crate::hooks::transcript_walker::is_real_user_turn` and `continue`
past synthetic turns rather than stop at them. A walker that stops
at any user turn — or filters only on array content — silently
fails the moment a Stop-hook refusal lands ahead of the real
invocation.

## The Closed Catalog of Synthetic User Turns

Claude Code emits user-role turns in three synthetic shapes alongside
user-typed prose. All three shapes carry `type:"user"` at the top
level, so a walker that discriminates only on `type` cannot tell
them apart from real user input.

| Shape | `message.content` | Marker | Examples |
|---|---|---|---|
| Tool-result wrapper | Array of `tool_result` blocks | `isMeta` absent / false | Tool-call results, slash-command expansions |
| Hook-injected feedback | String (e.g. `"Stop hook feedback:\n..."`) | `isMeta:true` | Stop-hook refusals, PreToolUse rejections |
| Compaction continuation | String (the carried-over summary text) | `isCompactSummary:true` (no `isMeta`) | Post-compaction continuation turn |

The compaction-continuation shape is the trap that string-only
filters miss: it has string `content` and NO `isMeta` field, so a
walker that filters only on `isMeta` treats it as a real user turn.
The dedicated `isCompactSummary` marker is the only field that
distinguishes it from user-typed prose.

Real user turns have string `content`, no `isMeta:true` field, AND
no `isCompactSummary:true` field. All three checks must pass — none
suffices alone.

## Real User Turns: Imperative vs Conversational Shapes

Real (non-synthetic) user turns split further into two classes —
the same `is_real_user_turn` discriminator covers both, but
`most_recent_user_message_since_skill_action` is the one walker
in the family that must distinguish them.

| Shape | Content begins with | Walker semantics |
|---|---|---|
| Conversational prose | Anything other than the slash-command tags | Captured as the candidate user message; consumer treats it as a halt trigger |
| Imperative slash-command input | `<command-message>` or `<command-name>` after `trim_start` | Filtered from candidate capture; not a halt trigger |

A slash-command-shape user turn is user-direction input — the
user is invoking a slash command, not conversing with the
model. Treating it as halt-trigger prose would re-arm
`_halt_pending` after every `/flow:flow-continue` and trap the
autonomous flow in a permanent voluntary-stop state. The
discrimination is consumer-specific: it lives in
`most_recent_user_message_since_skill_action` alone because
every other walker uses real-user-turn as a *boundary* (where to
stop scanning) rather than as a *conversation signal*.

Within imperative slash commands, `/flow:flow-continue` is the
universal resume directive. The walker additionally
**watermarks** preceding conversational prose to `None` when it
sees a `/flow:flow-continue` turn: a user who first paused with
prose and then typed `/flow:flow-continue` has answered their
own pause, so the next Stop event must fire Rule 1 (encouraging
refusal) rather than re-arming Rule 2 or a fresh conversation
pass-through. Every other slash command (e.g.,
`/flow:flow-abort`) filters from candidate capture but does NOT
watermark preceding prose — only `/flow:flow-continue` is the
resume directive, so a user who pauses with prose and then
aborts still has a legitimate conversational signal that must
remain visible to the predicate.

Cross-reference:
`.claude/rules/autonomous-phase-discipline.md` "Conversation
pass-through" carries the consumer-side picture of how the
walker's `Some`/`None` returns drive the three rules of
`check_autonomous_stop`.

## Why All Three Checks Are Required

A walker that filters only on `content.as_str().is_some()` catches
the tool-result-wrapper shape but misses the hook-injected feedback
shape entirely. The Stop-hook refusal turn that fires when an
autonomous flow receives a model-initiated turn-end carries
`isMeta:true` AND string content — the walker treats it as a real
user turn, halts, and the downstream predicate fails open.

The counter-example that motivates the dual check: a multi-step
utility skill (`flow-plan`) runs the
decompose sub-skill, the model returns mid-pipeline with a
text-only synthesis, the Stop hook refuses the turn-end, and the
refusal injects a `type:"user"` turn with string content and
`isMeta:true`. On the next Stop event, the
`check_in_progress_utility_skill` predicate calls
`most_recent_skill_since_user`. Without the `isMeta:true` filter,
the walker stops at the refusal turn, returns `None`, and the
predicate decides "no Skill since the user spoke" — fails open,
the model's text-only turn-end is permitted, the flow halts
mid-pipeline.

A walker that filters on both `content.as_str()` AND `isMeta`
still misses the compaction-continuation shape, because that turn
carries string content and no `isMeta`. After a mid-flow
compaction, Claude Code injects a `type:"user"` continuation turn
carrying the summary text. The autonomous-stop conversation
pass-through branch
(`most_recent_user_message_since_skill_action`) captures it as a
real conversational user message, sets `_halt_pending`, and latches
the flow into a permanent voluntary-stop state with no user-visible
signal that the backstop is gone. The `isCompactSummary:true`
filter closes this third surface.

The same shapes break every other walker downstream:

- `last_user_message_invokes_skill` (Layer 1 user-only-skill
  enforcement) — stops at the synthetic turn, never sees the real
  invocation, silently blocks legitimate Skill calls.
- `most_recent_skill_in_user_only_set` (Layer 2 carve-out for
  in-progress autonomous AskUserQuestion) — stops at the
  synthetic turn, never sees the assistant Skill call before it,
  the carve-out fails to fire, the user-confirmation prompt
  deadlocks.
- `recent_edit_blocked_on_shared_config` (shared-config
  carve-out for autonomous AskUserQuestion) — stops at the
  synthetic turn, never reaches the tool_result-wrapped user turn
  that carries the BLOCKED message, the carve-out fails to fire,
  the system-initiated confirmation prompt deadlocks.

## The Mechanical Contract

Every walker in `src/hooks/transcript_walker.rs` that encounters a
`type:"user"` turn at backward scan and needs to decide whether
the turn is a real user message MUST consult `is_real_user_turn`.
Walkers that look for the most recent REAL user turn `continue`
past synthetic turns; walkers that look for the most recent user
turn of a SPECIFIC synthetic kind (e.g.,
`recent_edit_blocked_on_shared_config` which needs the array-
content tool_result wrapper) may filter on the specific shapes
they consume but must still skip the unrelated string-content
synthetic shapes (hook-feedback AND compaction-continuation).

Two filtering patterns satisfy the contract:

- **Helper-based skip.** `if !is_real_user_turn(&turn) { continue; }`
  — used when the walker needs the real user turn. Skips the
  array-content, `isMeta:true`, AND `isCompactSummary:true` shapes.
- **Targeted skip.** Manually skip the string-content synthetic
  shapes via `is_meta_marker_present(turn.get("isMeta"))` AND
  `is_compact_summary_turn(&turn)` — used when the walker
  legitimately consumes array-content user turns (the shared-config
  carve-out is the canonical example: it inline-skips both
  string-content synthetic shapes while still examining the
  array-content tool_result wrapper).

A walker that inlines the discrimination from scratch is forbidden.
Inlining hides the contract from future readers and produces drift
the moment a new synthetic shape is added — the shared predicates
(`is_real_user_turn`, `is_meta_marker_present`,
`is_compact_summary_turn`) are the single point of update.

## How to Apply

**Authoring a new walker.** When designing a backward walker over
transcript JSONL that decides on user-role turn boundaries,
default to `is_real_user_turn`. Reach for the targeted skip
pattern only when the walker's purpose specifically requires
array-content user turns; document the choice in the walker's
doc comment.

**Modifying an existing walker.** Before changing the
user-boundary logic in any walker, identify which of the two
patterns the walker uses and preserve the discrimination
property. A change that filters only on `content.as_str()` (or
only on `isMeta`, missing the `isCompactSummary` shape) re-opens
the bypass surface and must be rejected.

**Adding a new walker callsite.** When a new hook or subcommand
calls one of the walkers, no action is needed — the walker
already discriminates correctly. The contract lives inside the
walker, not at the callsite.

## Enforcement

The rule is enforced primarily by the discipline of this file and
the integration test corpus in
`tests/hooks/transcript_walker.rs`. Per-walker regression tests
named
`<walker>_skips_hook_feedback_string_content_ismeta_true` and
`<walker>_skips_compact_summary_turn` lock the discrimination
property in for each walker. A future edit that removes the
`is_real_user_turn` call (or either targeted string-content skip)
trips the matching test.

The Review reviewer agent flags any new walker that inlines the
content/`isMeta`/`isCompactSummary` discrimination as a Real
finding — the shared predicates are the only sanctioned filter
path.

## Cross-References

- `.claude/rules/external-input-validation.md` — the parent
  discipline that says external input must be validated before
  invariant-bearing branches act on it. Transcript JSONL is the
  external input here; the synthetic-shape discriminator is the
  validator.
- `.claude/rules/security-gates.md` "Normalize Before Comparing"
  — the sibling discipline that walkers also follow when
  comparing user-supplied strings to gate values.
- `src/hooks/transcript_walker.rs` module doc — the source-local
  description of the JSONL turn shape and the real-vs-synthetic
  discrimination contract.
