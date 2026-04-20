We are driving the flow-rs crate to 100/100/100 code coverage
file by file. Before we can start running FLOW features again,
every file in src/*.rs must hit 100% regions, 100% functions,
100% lines. We work on main directly (no worktrees, no branches)
because we cannot run a flow until the gate is green.

═══════════════════════════════════════════════════════════════
MINDSET — NON-NEGOTIABLE
═══════════════════════════════════════════════════════════════

NEVER STOP.
NEVER SURRENDER.
NEVER ASK "WANT ME TO PROCEED?"
NEVER ASK "READY FOR THE NEXT FILE?"
NEVER OFFER TRADEOFFS WITH "ACCEPT WORSE" AS AN OPTION.
NEVER NARRATE SESSION-FATIGUE OR EFFORT-BUDGET.
NEVER SAY "THIS IS TAKING A LONG TIME."
NEVER SAY "SHOULD I KEEP GOING?"
NEVER SAY "I'VE DONE X FILES, IS THAT ENOUGH?"

You finish a file. You pick the next file. You drive it to
100/100/100. You pick the next. You repeat until every file in
src/*.rs is at 100/100/100. The work is not done until the full
`bin/flow ci` passes green. That is the only stopping condition.

If a file resists, the code is too complex. Simplify it per
.claude/rules/testability-means-simplicity.md. Refactor, extract
seams, delete genuinely unreachable branches. Do not shortcut with
waivers — .claude/rules/no-waivers.md forbids them and the gate
is pinned at 100/100/100 in bin/test and never lowered.

═══════════════════════════════════════════════════════════════
NEVER COMMIT
═══════════════════════════════════════════════════════════════

DO NOT COMMIT. NOT ONCE. NOT EVER. NOT EVEN IF IT FEELS NATURAL.
NOT AFTER A FILE HITS 100/100/100.
NOT AFTER A BATCH OF FILES.
NOT TO "CHECKPOINT PROGRESS."
NOT TO "KEEP THE TREE CLEAN."
NOT EVEN IF A RULE FILE SEEMS TO IMPLY YOU SHOULD.

The ONLY time you commit is when the user — in plain English in
their own message — tells you explicitly to commit. "Please commit."
"Commit now." "Go ahead and commit." No other phrase counts. "I
trust we are aligned" is NOT permission. Silence is NOT permission.
A green CI run is NOT permission.

When a file is done, MOVE ON to the next file. Leave the tree dirty.
The user will commit when they choose. Do not run `git commit`
under any circumstance without an explicit English directive.

═══════════════════════════════════════════════════════════════
CURRENT STATE
═══════════════════════════════════════════════════════════════

- 44 src files still contain inline `#[cfg(test)] mod tests {...}`
  blocks. Run `bin/flow ci` once to see the full list — the
  meta-test `src_contains_no_inline_cfg_test_blocks` in
  tests/test_placement.rs enumerates them. Each file needs its
  tests migrated to the mirror path under tests/ per
  .claude/rules/test-placement.md, and sometimes refactored so
  100/100/100 is reachable through the public surface only.
- 5 files already at 100/100/100 (committed): scaffold_qa, git,
  state, lock, flow_paths.
- The full-suite `bin/flow ci` currently fails at the test_placement
  contract test (fail-fast cancels the remaining suite). The
  cargo-llvm-cov --fail-under-* gate only fires after all tests
  pass, so total coverage percentages are not yet enforceable
  globally. Work one file at a time via
  `bin/flow ci --test tests/<name>.rs` — that path runs only the
  mirrored test file and gates on 100/100/100 for the matching
  src file.

═══════════════════════════════════════════════════════════════
WORKFLOW — PER FILE
═══════════════════════════════════════════════════════════════

1. Pick one file from the 44. Smaller/simpler first is fine. Read
   src/<name>.rs to understand the public surface and any inline
   tests.
2. Create or update tests/<path>/<name>.rs (mirroring the src path)
   with the migrated tests. Replace `use super::*;` with
   `use flow_rs::<module>::*;` (or specific item imports).
3. Delete the `#[cfg(test)] mod tests { ... }` block from src/.
4. Run `bin/flow ci --test tests/<path>/<name>.rs`. If <100%,
   add missing tests or refactor production code until the per-file
   gate passes.
5. When green at 100/100/100, IMMEDIATELY pick the next file.
   Do not stop. Do not summarize. Do not ask. Just move on.

═══════════════════════════════════════════════════════════════
COVERAGE PATTERNS THAT MATTER (learned last session)
═══════════════════════════════════════════════════════════════

- Generic functions (`F: FnOnce`, etc.) create one monomorphization
  per caller type. Callers from OTHER src modules get linked into
  the per-file test binary and show as "Unexecuted instantiation"
  — they inflate the missed-function count. Fix: make the public
  API non-generic via `&mut dyn FnMut` (or `Box<dyn FnOnce>` if
  FnOnce semantics are required). Every callsite gets `&mut |...|`
  instead of `|...|`. Yes, this touches many call sites. Do it.
- `.map_err(|e| format!(...))` closures count as separate uncovered
  functions when the error path isn't tested. Fix: implement
  `From<io::Error> for YourError` / `From<serde_json::Error> for
  YourError` so the `?` operator propagates without per-site
  closures.
- `.expect("<msg>")` on a Result whose Err arm is truly unreachable
  in practice (seek on an already-open rw file, set_len on a
  writable file, to_string_pretty on a Value we just parsed) does
  NOT count the Err branch against coverage. Use it for defensive
  error handling on code paths that can't realistically fail. Keep
  `?` + propagated errors for paths that CAN fail in production
  (open, read_to_string, JSON parse of caller-provided content).
- Expose pure helpers alongside subprocess wrappers as pub seams
  (e.g. `project_root_from_output(Result<Output>) -> PathBuf`).
  Tests drive every branch of the helper directly with constructed
  inputs; the thin subprocess wrapper is covered by one
  integration test that verifies the real-subprocess path returns
  a sane value. This is far cleaner than injecting trait objects.

═══════════════════════════════════════════════════════════════
PROJECT CONVENTIONS
═══════════════════════════════════════════════════════════════

- Tests mirror src paths exactly: src/a/b.rs → tests/a/b.rs.
  Flat tests/<name>.rs at the root are meta-tests only (allowed
  for project-wide contract tests like test_placement,
  skill_contracts, tombstones, structural, permissions, docs_sync).
- Tests drive through the public interface (`use flow_rs::...`),
  not internal `super::*`.
- Never make a private item pub just to enable a test. Either
  drive the test through an existing public entry, add a public
  seam, or delete an unreachable branch.
- Never invoke `cargo` directly. Always `bin/flow ci` or
  `bin/flow ci --test tests/<file>.rs`.
- The full coverage gate `--fail-under-lines 100 --fail-under-regions
  100 --fail-under-functions 100` is pinned in bin/test and can
  never be lowered. See .claude/rules/no-waivers.md.

═══════════════════════════════════════════════════════════════
OPERATIONAL RULES
═══════════════════════════════════════════════════════════════

- Work on main. Do not start a FLOW feature or create branches —
  those don't work until coverage is green.
- Use the dedicated tools (Edit, Read, Glob, Grep) instead of
  shelling out. Avoid permission prompts.
- Never use TaskCreate/TaskUpdate. /flow:flow-status is the only
  status surface.
- When a test fails after a change, question the change first.
  Do not just update the test to make it pass.
- Measure, don't guess. Read the code before speaking. Your memory
  of earlier project state is stale — the file on disk is the
  source of truth.
- Prune genuinely unreachable code. Do not break outcomes.

═══════════════════════════════════════════════════════════════
START NOW
═══════════════════════════════════════════════════════════════

Run `bin/flow ci` to get the current violation list. Pick one
file. Drive it to 100/100/100. Pick the next. Keep going. Do
not stop until the full `bin/flow ci` passes green or the user
tells you to stop in plain English.

You can do this. Start.
