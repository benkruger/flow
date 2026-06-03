# Transcript Walker Cap Selection

Walkers over the persisted Claude Code transcript JSONL declare
their lookback semantics by name. Three public wrappers in
`src/hooks/transcript_walker.rs` route every transcript file read:

- `read_full(path)` — uncapped. Loads the entire transcript.
- `read_recency_window(path)` — capped at `TRANSCRIPT_BYTE_CAP`
  (50 MB tail).
- `read_recent_turns(path)` — capped at
  `SHARED_CONFIG_BLOCK_BYTE_CAP` (4 MB tail).

The private `read_capped(path, cap)` is the seek-and-take primitive
both capped wrappers share. Direct calls to `read_capped` from
production code outside the three named helpers are forbidden.

## Why

A single cap value cannot serve every walker class. Phase-boundary
walkers and per-turn recency walkers have asymmetric costs:

- **Per-turn recency walkers** run on every Skill /
  AskUserQuestion / Stop event. They must stay bounded as a
  session's transcript grows past 100 MB. The 50 MB tail window
  comfortably covers the most recent ~10,000 turns — enough recency
  for user-only-skill detection, halt-pause detection, the
  bootstrap carve-out, etc. — while keeping per-turn latency
  predictable.
- **Phase-boundary walkers** run at most once per phase boundary
  (agent return, phase finalize, cleanup). A marker they search for
  — for example a `phase-enter --phase <p>` line — can sit
  arbitrarily far back in a long autonomous flow's transcript. A
  tail-bounded read silently misses the marker the moment the
  transcript exceeds the cap, so the walker draws the wrong
  conclusion from an incomplete view. The uncapped `read_full` path
  is correct here because the walker's rare invocation rate makes
  the memory cost (one full transcript load per boundary)
  acceptable.
- **Shared-config-block detection** runs on every blocked
  AskUserQuestion during autonomous phases. It only needs the
  latest assistant tool call and its paired tool_result. The 4 MB
  cap is even tighter than the per-turn recency window because the
  blocked-AskUserQuestion hot path adds latency to every blocked
  prompt — and the data the detection needs is structurally close
  to the file tail.

Choosing the wrong cap is silent: a phase-boundary walker on
`read_recency_window` misses markers on long flows; a per-turn
walker on `read_full` blows up the hot path on every event. Naming
the wrapper after its lookback semantics forces the choice into the
type signature where the wrong selection is visible at the
callsite.

## How to Apply

When adding a new walker or a new caller of an existing walker,
classify the lookback semantics first:

1. **Where is the marker the walker is looking for?**
   - Among the most recent ~1-2 turns since the user last spoke →
     `read_recent_turns`.
   - Among the most recent ~10,000 turns (last hour or two of an
     autonomous flow) → `read_recency_window`.
   - Possibly arbitrarily far back (across phase boundaries, across
     compaction events, anywhere in the session) → `read_full`.
2. **How often does the walker run?**
   - Per-turn (every Skill, every Stop, every AskUserQuestion) →
     must be `read_recency_window` or `read_recent_turns`. Never
     `read_full`.
   - Rare (per-agent-return, per-phase-finalize, per-cleanup) →
     `read_full` is acceptable when the marker may sit far back.
3. **Pick the matching wrapper.** Call the named helper directly.
   Never call `read_capped` from production code.

When extending the recency window cap (`TRANSCRIPT_BYTE_CAP`) or
the shared-config cap (`SHARED_CONFIG_BLOCK_BYTE_CAP`), update the
constant's doc comment to explain the trade-off the new value
represents. The cap names are intentionally abstract — callers
read the wrapper name, not the constant value.

## Enforcement

The contract is enforced by
`read_capped_only_called_inside_named_helpers` in
`tests/hooks/transcript_walker.rs`. The test reads
`src/hooks/transcript_walker.rs` source content and asserts:

- `read_full` body contains zero `read_capped(` calls (it wraps
  `fs::read_to_string`).
- `read_recency_window` body contains exactly one `read_capped(`
  call.
- `read_recent_turns` body contains exactly one `read_capped(`
  call.
- The total occurrence count of `read_capped(` across the file is
  exactly three (two helper bodies + the `fn read_capped(`
  signature). Any fourth occurrence is a direct caller outside the
  three named wrappers and fails the test.
- Every other `.rs` file under `src/` contains zero `read_capped(`
  occurrences. (The compile-time visibility check already prevents
  cross-module calls; the contract test makes the invariant
  explicit so a future change that exposes `read_capped` as `pub`
  and adds an outside caller trips here.)

## Cross-References

- `.claude/rules/external-input-path-construction.md` — the
  byte-cap discipline this rule is one application of. Every
  external file read must enforce a documented size cap on the
  hot path; `read_full` is the documented exception for
  phase-boundary walkers whose marker may sit far back.
- `src/hooks/transcript_walker.rs` module doc — names the
  consumers of each wrapper and the rationale for the three
  windows.
- `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
  in Contract Tests" — the bounded-slice pattern the contract
  test uses to count `read_capped(` occurrences inside each
  helper body.
