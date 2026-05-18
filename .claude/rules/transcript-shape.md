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

Claude Code emits user-role turns in two synthetic shapes alongside
user-typed prose. Both shapes carry `type:"user"` at the top level,
so a walker that discriminates only on `type` cannot tell them
apart from real user input.

| Shape | `message.content` | `isMeta` | Examples |
|---|---|---|---|
| Tool-result wrapper | Array of `tool_result` blocks | absent / false | Tool-call results, slash-command expansions |
| Hook-injected feedback | String (e.g. `"Stop hook feedback:\n..."`) | `true` | Stop-hook refusals, PreToolUse rejections |

Real user turns have string `content` AND no `isMeta:true` field.
Both checks must pass — neither suffices alone.

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

## Why Both Checks Are Required

A walker that filters only on `content.as_str().is_some()` catches
the tool-result-wrapper shape but misses the hook-injected feedback
shape entirely. The Stop-hook refusal turn that fires when an
autonomous flow receives a model-initiated turn-end carries
`isMeta:true` AND string content — the walker treats it as a real
user turn, halts, and the downstream predicate fails open.

The counter-example that motivates the dual check: a multi-step
utility skill (`flow-plan`, `flow-decompose-project`) runs the
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

The same shape breaks every other walker downstream:

- `last_user_message_invokes_skill` (Layer 1 user-only-skill
  enforcement) — stops at the refusal, never sees the real
  invocation, silently blocks legitimate Skill calls.
- `most_recent_skill_in_user_only_set` (Layer 2 carve-out for
  in-progress autonomous AskUserQuestion) — stops at the
  refusal, never sees the assistant Skill call before it, the
  carve-out fails to fire, the user-confirmation prompt
  deadlocks.
- `recent_edit_blocked_on_shared_config` (shared-config
  carve-out for autonomous AskUserQuestion) — stops at the
  refusal, never reaches the tool_result-wrapped user turn that
  carries the BLOCKED message, the carve-out fails to fire, the
  system-initiated confirmation prompt deadlocks.

## The Mechanical Contract

Every walker in `src/hooks/transcript_walker.rs` that encounters a
`type:"user"` turn at backward scan and needs to decide whether
the turn is a real user message MUST consult `is_real_user_turn`.
Walkers that look for the most recent REAL user turn `continue`
past synthetic turns; walkers that look for the most recent user
turn of a SPECIFIC synthetic kind (e.g.,
`recent_edit_blocked_on_shared_config` which needs the array-
content tool_result wrapper) may filter on the specific shape
they consume but must still skip the unrelated hook-feedback
shape.

Two filtering patterns satisfy the contract:

- **Helper-based skip.** `if !is_real_user_turn(&turn) { continue; }`
  — used when the walker needs the real user turn. Skips both
  array-content AND `isMeta:true` shapes.
- **Targeted skip.** Manually check `content.as_str().is_some() &&
  isMeta == Some(true)` to skip ONLY the hook-feedback shape —
  used when the walker legitimately consumes array-content user
  turns (the shared-config carve-out is the canonical example).

A walker that inlines the discrimination is forbidden. Inlining
hides the contract from future readers and produces drift the
moment a new synthetic shape is added — the helper is the single
point of update.

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
only on `isMeta`) re-opens the bypass surface and must be
rejected.

**Adding a new walker callsite.** When a new hook or subcommand
calls one of the walkers, no action is needed — the walker
already discriminates correctly. The contract lives inside the
walker, not at the callsite.

## Enforcement

The rule is enforced primarily by the discipline of this file and
the integration test corpus in
`tests/hooks/transcript_walker.rs`. Per-walker regression tests
named
`<walker>_skips_hook_feedback_string_content_ismeta_true` lock the
discrimination property in for each walker. A future edit that
removes the `is_real_user_turn` call (or the targeted
hook-feedback skip) trips the matching test.

The Review reviewer agent flags any new walker that inlines the
content/isMeta discrimination as a Real finding — the helper is
the only sanctioned filter path.

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
