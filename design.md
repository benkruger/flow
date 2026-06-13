# FLOW v3 — Design

Living spec. Grows as `pressure-points.md` items are decided.
Status: early — core flip and language locked; enforcement, state, and planning
surfaces still open.

## The core flip

v2: the model is the runtime. Skills (markdown) are the program, Claude is the
interpreter, and Rust/hooks police the interpreter forensically.

v3: **FLOW is a program that calls Claude.** The orchestrator owns the loop —
phase and task sequencing is literal code; Claude executes inside one line of it
as a fresh, bounded session per task. Trust comes from process separation, not
from policing a context window.

```ts
for (const task of plan.tasks) {
  await runClaudeSession(taskPrompt(task), { cwd: worktree }); // Claude works
  if (!await ci()) await retryWithFixer(task);                 // we verify
  await commitAndPush(task);                                   // we commit
}
await runReviewAgents();
await mergeAfterApproval();
```

Claude never sees the loop, can't skip an iteration, can't declare a phase done.
Turn policing becomes post-condition checking: after each worker session the
orchestrator verifies measurable outcomes (CI green, test exists, diff non-empty),
retries bounded, fails loudly.

## Locked decisions

| Date | Decision |
|------|----------|
| 2026-06-12 | **Control inversion** (above) |
| 2026-06-12 | **TypeScript on Bun.** Bun test runner (fast TDD), V8 coverage + Stryker mutation testing, Biome/ESLint, flagship Agent SDK, `bun build --compile` static binary. Discipline: Node-compatible code; no Bun-only APIs except test runner + compiler — Node is the escape hatch. |
| 2026-06-12 | **Rust retired.** Its anti-tamper rationale (compiled binaries the model can't backdoor) dissolved — in v3 the model never holds enforcement or the program counter. |
| 2026-06-13 | **Workers get no Bash tool (B-pure).** Finite typed toolset only; missing capability = structured failure, never a shell escape. See "Worker toolset" below. Considered + dropped full-permission Docker sandbox (keep native, zero-install). |
| 2026-06-13 | **Transcript walking deleted entirely.** Human talks to the trusted orchestrator directly (TUI/CLI); user intent never passes through the model, so there's nothing to walk. User-only actions are structurally user-only (capability absent). Kills coupling to Claude Code's internal JSONL formats. |
| 2026-06-13 | **Auto/manual autonomy system yanked entirely.** It meant "pause the shared conversation loop and ask me"; inversion deletes the shared loop. Orchestrator just runs. Deleted: `.flow.json` skills config, `resolve-skill-mode`, `continue_action`, `_auto_continue`, commit/continue axes, prime's preset picker. Per-task review → live TUI stream + PR at merge. |
| 2026-06-13 | **Authorization = real prompts + general blocked/attention state.** No markers/phrase-matching. Flow parks on a wall, surfaces in TUI, human resolves over socket. See "Blocked / needs-attention" below. |
| 2026-06-13 | **Sole config knob: merge auto vs require-approval.** `flow start` flag (`--auto-merge`; default require-approval). No repo config file. Optional per-user default (`FLOW_MERGE=auto` env / global). Base branch auto-detected; issue/repo are args. |
| 2026-06-13 | **cwd-as-identity deleted.** Orchestrator process holds branch/worktree as variables for its life; passes explicit paths to everything it spawns. Gone: drift guards, resume-anchor markers, mono-repo path mangling. |
| 2026-06-13 | **State god-object split by owner+durability.** Live state = process memory; durable progress = event journal + git commits; v2 hook flags all deleted; telemetry/notes = journal events. See "State model" below. |
| 2026-06-13 | **Slack removed entirely.** Noisy, half-implemented; not replaced. |
| 2026-06-13 | **Plan handoff: no change.** Plan stays in the decomposed GitHub issue (flow-plan output unchanged); sentinels stay as a trivial delimiter; the binary parses it robustly. v2's byte-extraction anxiety was skill-era; a binary consumer dissolves it without new formats/artifacts. |
| 2026-06-13 | **GitHub labels kept; orchestrator owns lifecycle.** Flow In-Progress (cross-engineer WIP signal) and decomposed/vanilla type labels stay. Orchestrator applies at start, removes at complete/abort; crash handled by supervision (abort/resume reconcile). Removes v2's scattered per-error-path cleanup invariants. (Triage In-Progress = planning skill, out of scope.) |
| 2026-06-13 | **Testing strategy unchanged.** 100% coverage of everything (no waivers); test files mirror source (`tests/<path>` ↔ `src/<path>`), each test file = 100% of its mirrored source; tombstones for deletions; test-placement discipline. Only `--clean` + phantom-misses dance vanish (Rust/llvm-cov artifacts; Bun V8 coverage has no equivalent). |
| 2026-06-13 | **Telemetry/cost identical to v2.** Nothing dropped: per-phase + total tokens, per-model breakdown, dollar cost (bundled price table), month-to-date, account-window 5h/7d %, all display surfaces (TUI + PR body). Only invisible change: token usage from SDK return value instead of transcript JSONL parsing. |
| 2026-06-13 | **Orchestrate/overnight mode yanked from v3 v1.** Never used in v2; full prune (orchestrate-state, orchestrate-report, queue, TUI Orchestration tab). Re-add later as a clean addition — trivial under the per-flow-process model ("start N flows" = N processes, no singleton lock, no TOCTOU). |

## Product split: conversation vs execution

- **Planning is a conversation** — explore / plan / triage stay as Claude Code
  skills, human in the loop. (These were always the least-scaffolded skills.)
- **Execution is a program** — Start → Code → Review → Learn → Complete runs
  under the orchestrator. The plan gate (plan-reviewer pass) is the
  highest-leverage checkpoint in the system: the only intervention users need
  downstream is abort-because-plan-was-bad.

## Process model

One detached orchestrator process per flow. No central daemon (single point of
failure, install weight, fights the unobtrusive tenet).

- `flow start 123` — spawns detached orchestrator (survives terminal close),
  drops into TUI attached to it. `q` detaches; flow keeps running.
- `flow tui` — discovers running flows (journal + pidfile), attaches to any;
  pause / approve / abort over a local socket.
- `flow resume <branch>` — replays the event journal, continues from the last
  committed task. Half-done in-flight work is discarded worktree dirt; the
  commit is the unit of progress.
- `flow abort <branch>` — kill the process tree, then orchestrator-owned cleanup
  (close PR, remove worktree, delete state). Ctrl-C semantics. No halt flags,
  no two-exit grammar.

Workers are child subprocesses of their orchestrator. Durable state = git
commits + an append-only event journal under `.flow-states/<branch>/`.

## Workers (Claude sessions)

- Fresh session per task / per agent. Context never grows beyond a task;
  compaction recovery deletes itself.
- Orchestrator curates inputs (plan, task, diff slices) — cognitive isolation
  becomes the default, not a discipline.
- Permissions via SDK `canUseTool` callback: structured policy code in the
  orchestrator replaces bash string forensics. Workers are never given the
  capabilities the orchestrator reserves (commit, merge, phase advance).
- Agent outputs use SDK structured output with schemas — incomplete output is
  mechanically detectable (replaces END-OF-FINDINGS sentinel strings).
- Residual cheat surface: a worker weakening tests/`bin/*` inside its worktree
  to fake green. Handled by protected-path denials in the permission callback +
  Review (as today).

## Worker toolset (no Bash)

Workers get a finite, typed toolset — never a Bash tool. The arms race
(`validate_pretool`, 97KB of shell-string parsing) doesn't shrink; it ceases to
exist, because the model never authors a shell string.

Starter set (Code phase):
- `Read` / `Write` / `Edit` — worktree-scoped; protected paths (`bin/*`, the task's
  own test file, CI config) denied at the tool layer
- `Grep` / `Glob` — read-only search (native)
- `RunTests(filter?)` / `Lint(fix?)` / `Format()` / `Build()` — TS functions that
  `spawn("bin/test", [filter])` etc.
- `GitDiff()` / `GitStatus()` — read-only

Mechanism: a tool like `RunTests(filter)` passes `filter` as a single argv element
to `spawn` — never interpolated into a shell. `RunTests("x; rm -rf ~")` runs
`bin/test` with one literal garbage argument; there is no shell to interpret the
`;`. Injection has no surface.

The `bin/*` stubs stay bash, project-owned, and may do arbitrary work inside
(migrations, codegen, fixtures). v2's "every project owns its toolchain" is fully
preserved — only the model's ability to *author* invocations is removed.

Per-phase toolsets fall out naturally: review agents get Read/Grep/Glob + RunTests
and no Write; the adversarial agent gets a scoped test-write tool. v2's awkward
special-cases (adversarial needing a write path) become just different typed lists.

Capability cliff: when a task needs something no tool covers, the worker returns a
**structured failure** ("needed X, no tool") that surfaces to the user — a signal to
add a tool, never a silent hole and never a Bash escape. The toolset converges on
what's actually needed.

Open risk to validate early: Claude is trained heavily on Bash; a no-Bash worker
*might* code slightly worse than a shell-enabled one. Empirical, testable.

## State model

No god-object `state.json`. State splits three ways by owner and durability:

- **Live flow state** — the orchestrator is a running process, so it just *has*
  variables (branch, worktree, current phase/task, model handles). No serialization
  for in-process use, no single file lock, no "re-read state each turn."
- **Durable progress** — an append-only **event journal** per flow
  (`.flow-states/<branch>/`) plus git commits. Enough to replay after a crash:
  `flow resume` reads the journal and continues from the last committed task.
- **Transient v2 hook flags** (`_halt_pending`, `_continue_pending`, `_blocked`,
  `_continue_context`) — **none survive.** They were messages-in-a-bottle between
  stateless hook invocations; a living process has nothing to coordinate that way.

Telemetry and notes are emitted as journal events, not core state. The journal is
the durable source of truth; the running process is the live form; either
reconstructs the other.

## Blocked / needs-attention (the only human-in-the-loop surface)

The orchestrator runs fully autonomously until it hits a wall, then parks the flow
(`blocked: {kind, detail, since}` in the journal) and surfaces it in the TUI as
"⚠ needs attention." The TUI is the single place a human touches a running flow —
and the attention queue is empty when everything's flowing. A human resolves via a
socket message to the parked orchestrator, which then resumes.

Two kinds of wall:
- **Policy block (configured):** merge, when running require-approval. The *only*
  configurable stop.
- **Exception block (always, regardless of config):** worker wants a denied/protected
  edit (e.g. `Cargo.toml`, `bin/*`), task still red after N retries, capability cliff
  (B-pure structured failure), unresolvable merge conflict, expired `gh` auth.

Resolution actions are kind-specific: merge → Approve / Reject; protected edit →
Allow-once / Deny; failed task → Retry / Abort; capability gap → Abort (and it's the
signal to add a tool). Detached/overnight: the flow stays parked durably until a
human attaches and resolves — it never blocks forever in-process and never
auto-proceeds past a wall.

## Configuration (there is almost none)

One knob in the whole system: **does merge auto-proceed, or park for approval?**

- `flow start 123` → require-approval (default — the irreversible, outward-facing
  action is the explicit opt-in).
- `flow start 123 --auto-merge` → full send.
- Default is a binary constant. Optional per-user standing default via `FLOW_MERGE=auto`
  (env) or a one-line global user config — **never** a repo file (v2 `.flow.json` was
  already git-excluded/per-engineer, so merge policy was never shared team state).

Not config: Slack webhook (env secret), base branch (auto-detected), issue/repo
(arguments).

## Observability

TUI is the control surface and is fed natively (the orchestrator IS the source
of truth — no state-file scraping, no polling):

1. **Flow level** — phase timeline, task m/n, PR link, elapsed, cost.
2. **Task level** — current task, last gate result, per-commit diff stats.
3. **Worker level** — live stream of the active session's tool calls / files
   edited / test output (SDK event stream). New capability vs v2.

Headless mode logs; TUI attaches later. PR-body rendering survives as an
orchestrator output. (Slack removed — see decisions table.)

## What survives from v2 (the preserve-list)

Stronger under inversion, because the process that owns execution enforces them:

- Model never computes timestamps / counters / gate decisions
- CI as hard gate inside the commit, not after
- Start-gate serialization: one green, dep-current base; O(1) repair via fixer
- Worktree isolation; trunk never touched directly
- Cognitive isolation of review agents (reviewer context-rich; pre-mortem /
  adversarial / documentation diff-only); parent never fabricates findings
- Bounded-retry fixer pattern (3 attempts) generalized to post-condition checks
- User-only actions stay user-only — now trivially: the orchestrator owns the UI,
  so "user must approve merge" is a real prompt, not transcript archaeology
- Corrections route durably; learnings compound (Learn phase mechanism TBD)

## Open questions (tracked in pressure-points.md)

- #5–7: enforcement layer — exact `canUseTool` policy shape; what (if anything)
  still needs hooks inside worker sessions
- #8–12: state & identity — journal schema, what replaces the god-object state
  file, plan handoff format (sentinels?), GitHub-label coordination
- #13: testing strategy for v3 itself (coverage targets, mutation testing role)
- #15: telemetry/cost capture via SDK instead of transcript parsing
- (#16 orchestrate/overnight — YANKED from v3 v1; re-add later)
- (Migration — N/A. The decomposed GitHub issue is the only contract and it's
  unchanged, so v2-produced issues feed v3's `flow start` as-is. 100% backwards
  compatible at the interface that matters. `.flow.json` / `bin/*` stubs / hooks
  are not a concern.)

**All 16 pressure points decided; no open design questions remain.**
Next phase: turn this spec into a build plan.
