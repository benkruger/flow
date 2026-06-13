# FLOW v-next — Pressure Point Inventory

Source: full deep-read of ~/code/flow (v2.6.1) on 2026-06-12.
Status legend: `open` / `discussed` / `decided: <direction>`

**LOCKED (2026-06-12): TypeScript on Bun.** Rationale: fastest TDD cycle (Bun test
runner), dense code with load-bearing types, V8 coverage (statements/branches/
functions/lines) + Stryker mutation testing, Biome/ESLint, flagship Agent SDK,
`bun build --compile` single static binary preserves zero-dependency install.
Discipline: stay Node-compatible — no Bun-only APIs except test runner + compiler,
so Node is the escape hatch for runtime edges. Rust's anti-tamper rationale dissolved
with control inversion (trust = process separation, not compilation).

**LOCKED (2026-06-12): control inversion.** FLOW becomes an orchestrator program
(TS/Bun + Agent SDK) that owns the loop — phase/task sequencing is a literal `for`
loop in code; Claude runs inside one line of it as a fresh bounded session per task.
Orchestrator runs all gates itself. Planning skills stay conversational in Claude
Code. TUI is the control surface. No mid-task interruption; abort = kill process
tree + cleanup. Plan gate becomes the highest-leverage checkpoint.

See `design.md` for the living spec.

## A. The model is the runtime (root cause #1: orchestration lives inside the conversation loop)

1. **Skills as prose state machines** — `decided: dissolved by control inversion (loop variable in code)`
   Locked step sequences, `--continue-step` self-invocation, resume counters
   (`code_task`, `review_step`…), HARD-GATE language, renumbering audits.
   The model interprets the program, so the program must be defended from its interpreter.

2. **Stop-hook turn-taking war** — `decided: turn policing → post-condition checks (CI green? test exists?) + bounded retries. Residue: subtle cheating stays Review's job`
   Two-exit halt model, `_halt_pending`, stall counters, performative-pause policing.
   Exists only because FLOW doesn't own the execution loop; the conversation does.

3. **Contract tests pinning prose** — `decided: prompts become typed template functions, tested as code + snapshots. Residue: small contract surface for remaining conversational planning skills`
   365KB `skill_contracts.rs` asserting step adjacency, gate wording, banners, marker
   strings. Editing a skill is high-friction because its text is load-bearing.

4. **Sub-agent truncation protocol** — `decided: whole-flow context growth + compaction recovery die; diff slicing survives as orchestrator code; END-OF-FINDINGS sentinel → SDK structured output with schema`
   `END-OF-FINDINGS` markers, Class 0–3 recovery, partition-by-family diff slices.
   Context-window management implemented in skill prose.

## B. Enforcement arms race (root cause #2: the model is untrusted but is the only actor, so trust is reconstructed forensically)

5. **validate_pretool** — `decided: workers get no Bash tool — typed parameterized tools only (B-pure)`
   97KB, 11 layers of bash string parsing: quote-state scanners, wrapper stripping,
   escape-hatch basename sets. Open-ended arms race against shell syntax.
   → Workers get a finite typed toolset (Read/Write/Edit worktree-scoped, Grep/Glob,
   RunTests(filter)/Lint()/Format()/Build(), read-only GitDiff/GitStatus). No Bash
   tool. Tools are TS functions that `spawn("bin/test",[filter])` — the model authors
   a typed arg, never a shell string, so injection has no surface (no shell to parse).
   bin/* stubs stay bash, project-owned, arbitrary work inside; only the model's
   ability to *author* invocations is removed. Per-phase toolsets fall out naturally
   (review agents: no Write; adversarial: scoped test-write tool). Missing capability
   = structured failure surfaced to user (backlog signal), never a Bash escape.
   The entire validate_pretool string-parsing surface ceases to exist.
   NOTE: considered full-permission Docker sandbox; dropped — keeping native, no
   containers (preserves v2 zero-install tenet). Integrity threats (faking green via
   test-tampering) are NOT a sandbox concern anyway — handled by protected-path tool
   denials + orchestrator running real gates + Review.

6. **Transcript walking as trust anchor** — `decided: deleted entirely, no replacement`
   User-typed slash commands recognized by matching Claude Code's internal JSONL
   emission shapes (`<command-message>` variants, `isMeta`, `isCompactSummary`).
   Breaks when the harness changes format; `/flow-changelog-audit` exists to watch for it.
   → Pure deletion. In v3 the human talks to the trusted orchestrator directly
   (TUI keypress / CLI), so user intent never passes through the model — there's no
   transcript to walk. User-only actions are structurally user-only: a worker has no
   path to `flow abort` or the orchestrator socket (capability absent, not gated).
   Kills the tightest coupling to Claude Code internals; `/flow-changelog-audit`
   shrinks to tracking the public SDK. No residue.

7. **Custom authorization plumbing** — `decided: real prompts + general blocked/attention state`
   Single-use fail-closed markers (merge approval, shared-config) keyed to exact
   user-typed phrases. Hand-rolled permission system on top of Claude Code's.
   → The markers were "ask + remember once" without a UI. v3 owns a UI, so it's
   ordinary. Generalized: a flow runs until it hits a wall, then parks
   (`blocked: {kind, detail, since}`) and surfaces in the TUI as "needs attention";
   a human resolves it (socket msg → orchestrator resumes). Two kinds:
   - Policy block (configured): merge, when require-approval. The only configurable stop.
   - Exception block (always): worker wants a denied/protected edit, task red after N
     retries, capability cliff (B-pure structured failure), unresolvable conflict,
     expired gh auth. Resolution actions are kind-specific (Approve/Allow-once/Retry/Abort).
   Detached/overnight: stays parked durably until a human attaches. No phrase-matching,
   no sha256 markers, no fail-closed file parsing.

## C. Identity & state fragility (mostly follows from A + B)

8. **cwd as identity** — `decided: process IS the flow; identity held as variables`
   Branch detected from cwd, cwd-drift guards in every mutator, resume-anchor markers,
   mono-repo `relative_cwd` mangling. Documented wrong-state-file failure modes.
   → The orchestrator process holds branch/worktree as plain variables for its whole
   life and passes explicit paths to everything it spawns. Nothing detects cwd.
   Gone: drift guards, resume-anchor markers, the whole recovery chain.

9. **State file as god object** — `decided: split by owner+durability; Slack removed; flags gone`
   ~40 top-level fields mixing identity, progress counters, transient hook flags,
   telemetry snapshots, notes, Slack metadata; many writers under one file lock.
   → Split by who owns it and how durable it must be:
   - Live flow state = orchestrator process memory (it's running; it just has variables).
     No serialization for in-process use, no single lock, no "re-read each turn".
   - Durable progress = append-only event journal + git commits. Enough to replay after crash.
   - Transient hook flags (_halt_pending, _continue_pending, _blocked, _continue_context)
     — NONE survive. They were messages-in-a-bottle between stateless hooks; a living
     process has nothing to coordinate that way.
   - "Needs attention" is NOT a flag: it's the orchestrator's own control flow parked at
     a wall — journal record (durable) + process parked on socket (live); either
     reconstructs the other. Resume re-parks at the same wall, never auto-proceeds.
   - **Slack system removed entirely** (noisy, half-implemented, bad idea — not replaced).
   - Telemetry/notes — derived or emitted as journal events, not core state.

10. **Sentinels in issue bodies** — `decided: NO CHANGE — plan stays in the decomposed issue`
    `FLOW-PLAN-BEGIN/END` HTML comments as a data channel inside GitHub prose;
    5-attempt validation loops; rules forbid skills from mentioning the literal strings.
    → The v2 fragility (byte-extraction "exactly twice", 5-retry validation,
    forbidden-mention rule) was skill-era anxiety: a Claude skill both wrote and
    consumed the plan. In v3 the consumer is the binary. A binary parsing a known
    region of an issue body is just robust — the anxiety evaporates without changing
    anything. flow-plan output unchanged; plan lives in the decomposed GitHub issue;
    sentinels stay as a trivial delimiter; `flow start #N` fetches + parses.
    No JSON, no new artifact, no moved data. (Rejected an over-engineered JSON-schema
    proposal — see simplicity tenet.)

11. **Autonomy config chain** — `decided: yanked entirely`
    `.flow.json` → copied into state → `resolve-skill-mode` → `continue_action` parsed
    by prose. Chain integrity is itself a rule because it drifted.
    → The whole auto/manual two-axis system is removed. "Manual" meant "pause the
    shared conversation loop and ask me" — and control inversion deletes the shared
    loop, so the concept is meaningless. The orchestrator just runs. Per-task diff
    review is replaced by the live TUI worker stream + the PR at merge.
    Deleted: `.flow.json` skills config, `resolve-skill-mode`, `continue_action`,
    `_auto_continue`, commit/continue axes, flow-prime's preset picker.
    Sole surviving knob: merge auto vs require-approval — a `flow start` flag
    (`--auto-merge`; default require-approval, so the irreversible action is the
    explicit opt-in), NOT a repo file (v2 `.flow.json` was already git-excluded/
    per-engineer, so it was never shared team state). Optional per-user default via
    env/global (`FLOW_MERGE=auto`). No repo config file anywhere.

12. **GitHub labels as distributed state** — `decided: keep labels; orchestrator owns lifecycle`
    Flow In-Progress, Triage In-Progress, decomposed/vanilla routing;
    label-cleanup invariants on every error path.
    → Labels stay — Flow In-Progress is the needed cross-engineer WIP signal;
    decomposed/vanilla are real issue-type metadata. The v2 problem was scattered
    "remove the label" cleanup invariants across skill error paths (dead flow → sticky
    label lies). Fix: the orchestrator owns the label lifecycle (apply at start, remove
    at complete/abort); crash/sticky handled by the same supervision as everything else
    (abort kills tree + cleans up incl. label; resume reconciles). Label stays, scattered
    cleanup goes. (Triage In-Progress = flow-triage-issue planning skill, out of scope
    for the execution lifecycle.)

## D. Infrastructure weight (independent of A–C, negotiable)

13. **Coverage machinery** — `decided: testing strategy UNCHANGED; only llvm-cov artifacts vanish`
    100/100/100 per-file gates, Layer 11 redirection, phantom-misses `--clean` recovery,
    179KB tombstone corpus. Much of it fights llvm-cov quirks rather than regressions.
    → Strategy is preserved wholesale: 100% coverage of everything (statements/branches/
    functions/lines), no waivers/no escape hatch; test files mirror source files
    (tests/<path> ↔ src/<path>), each test file = 100% of its mirrored source; tombstones
    for deletions; test-placement discipline. The ONLY things that disappear are `--clean`
    and the phantom-misses dance — pure Rust/llvm-cov artifacts that don't exist under
    Bun's V8 coverage. Not a strategy change, just dead weight that was never the point.

14. **Binary distribution** — `decided: bun build --compile per platform`
    Committed 7MB darwin-arm64 binary, hybrid source/prebuilt dispatch, auto-rebuild.
    Single-platform, binary churn in git. → Single static binary from Bun; no hybrid
    dispatch, no source-fallback compilation; cross-platform targets available.

15. **Telemetry duplication** — `decided: functionality IDENTICAL to v2; only the source changes`
    Token/cost capture by parsing transcripts + `rate-limits.json`, hardcoded pricing
    table. Re-derives what the harness knows.
    → Nothing dropped. ALL v2 telemetry preserved exactly: per-phase + total tokens,
    per-model breakdown, dollar cost, month-to-date reconciliation, account-window
    5h/7d %, every display surface (TUI cost panel + PR body). Bundled price table
    stays (= v2 pricing.rs); unknown-model handling stays (the "—"/partial markers).
    ONLY change is invisible plumbing: per-call token usage comes from the SDK return
    value instead of parsing transcript JSONL; account-window % keeps its source (or
    SDK if exposed). Same numbers, robust source, no transcript coupling.

16. **Orchestrate singleton** — `decided: YANKED entirely (never used); re-add later`
    JSON-state "lock" with acknowledged TOCTOU window; sequential-only overnight runs.
    → Full prune. Never used in the wild. Removed from v3 scope; can be re-architected
    later as a clean addition (trivial now: "start N flows" = N independent processes,
    no special sequential mode, no singleton lock, no TOCTOU). All of orchestrate-state,
    orchestrate-report, the queue, the TUI Orchestration tab — gone for v3 v1.

## Invariants any new architecture must preserve (from FLOW's own docs)

- Model never computes timestamps/counters/gate decisions
- Unforgeable evidence over model self-report (agent launches, user-typed commands)
- CI as hard gate inside the commit, not after it
- Start-gate serialization: one green, dep-current base; O(1) repair
- Cognitive isolation of review agents; parent never fabricates findings
- Two-tier authorization: user-only actions stay user-only
- Worktree isolation; trunk never touched directly
- Corrections (flow-note) route durably; learnings compound
- Autonomy from config, not flags
