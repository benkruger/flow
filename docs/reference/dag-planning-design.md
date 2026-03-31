# DAG-Enhanced Planning — Design Document

## Context

FLOW's Plan phase (Phase 2) produces a linear task list. For complex
features with many moving parts, this linear approach can miss hidden
dependencies between tasks, produce suboptimal ordering, and overlook
aspects of the problem that only surface when you explicitly map
relationships.

**Inspiration**:
[mkw-DAG-architect](https://github.com/matt-k-wong/mkw-DAG-architect)
is a Claude Code skill that decomposes problems into Directed Acyclic
Graphs — nodes with explicit dependencies, parallel branches, and
topological execution order. It identifies hidden dependencies and
ensures comprehensive coverage that linear reasoning misses.

**Goal**: Enhance FLOW's planning methodology to produce better-ordered,
more comprehensive task lists — without changing the Code phase or
breaking FLOW's safety model.

## What mkw-DAG-architect Does

A Claude Code skill (pure Markdown + XML templates, zero dependencies)
with a 5-phase workflow:

1. **Activation** — user triggers with `decompose: [goal]`
2. **Impact Preview** — assesses whether decomposition adds value
   (evaluates step count, real dependencies, parallelizable branches;
   returns verdict from HIGH VALUE to SKIP)
3. **Planning** — full XML DAG visualization: nodes with types
   (research, analysis, synthesis, validation, creative, decision),
   explicit dependency edges, parallel markers, cycle validation
4. **Execution** — nodes execute in topological order with quality
   scoring
5. **Synthesis** — results merge, contradictions resolve, final output
   compares to what linear reasoning would have missed

Key design: explicit dependencies + topological ordering + parallel
branch resolution + contradiction handling.

**Tech stack**: Pure bash install, SKILL.md (~12KB), 4 XML templates
(code-dag, research-dag, strategy-dag, generic), MIT license.

## What FLOW's Plan Phase Does Today

A 3-step workflow inside Claude Code's native plan mode:

1. **Feature description** — reads prompt from state file, fetches
   referenced GitHub issues
2. **Explore and write plan** — explores codebase, designs approach,
   writes plan file with: Context, Exploration, Risks, Approach, Tasks
   (linear, TDD-ordered)
3. **Store and complete** — saves plan file path in state, completes
   phase, exits plan mode

The plan file is Markdown at `.flow-states/<branch>-plan.md`. The Code
phase reads it and executes tasks one at a time with TDD cycles and CI
gates.

## Options Considered

### Option A: DAG as Planning Methodology (Recommended)

Enhance Plan Step 2 with DAG decomposition as a thinking tool. The DAG
informs task ordering and coverage, but the plan file stays linear.
Code phase unchanged.

**Changes**: `flow-plan/SKILL.md` gains ~2-3KB of enhanced
instructions, plan file gains optional "Dependency Graph" section.

### Option B: DAG-Structured Plan + Parallel Code Execution

Plan produces a DAG with dependency annotations. Code phase executes
independent branches in parallel via Agent tool.

**Changes**: Both `flow-plan/SKILL.md` and `flow-code/SKILL.md`
rewritten. New lib scripts for DAG parsing. State file schema changes.

### Option C: Install Alongside, Use Optionally

No FLOW changes. Install mkw-DAG-architect as a companion skill. Users
invoke `decompose:` during plan mode when they want it.

**Changes**: Nothing in FLOW.

## Detailed Comparison: Option A vs Option B

<!-- markdownlint-disable MD013 -->

| Dimension | A: DAG as planning tool | B: DAG + parallel execution |
|---|---|---|
| **Scope of change** | `flow-plan/SKILL.md` Step 2 gains ~2-3KB | Both skills rewritten, new lib scripts, state schema changes |
| **Plan file format** | Same linear task list + optional dependency graph section | Tasks gain `depends_on` fields, parallel group annotations |
| **Code phase** | Unchanged — one task, one TDD cycle, one CI gate | Fundamentally different — parse DAG, launch parallel agents, merge work, handle conflicts |
| **Error recovery** | Same as today — task fails, fix it, continue | Agent fails mid-graph — roll back others? Wait? What if they edited the same file? |
| **TDD ordering** | Clear — test before implementation, linear | Ambiguous — parallel tasks A and B: which tests run when? |
| **Review model** | One diff at a time, fully reviewable | Multiple diffs landing simultaneously from agents that can't see each other's work |
| **Merge conflicts** | Impossible (sequential execution) | Likely — two agents independently editing shared modules |
| **State tracking** | No new fields | Per-task completion, dependency graph, parallel execution state |
| **Risk level** | Low — worst case, DAG analysis is unhelpful and linear plan still works | High — parallel execution bugs are hard to reproduce and debug |

<!-- markdownlint-enable MD013 -->

### Why Option B Is Wrong for FLOW

The bottleneck in feature development is **understanding what to build
correctly**, not execution speed. DAG decomposition helps understanding
(Option A). Parallel execution helps speed (Option B) but undermines
the safety model:

- **FLOW's one-task-at-a-time model is a feature, not a limitation.**
  Each task gets a TDD cycle, a CI gate, and a reviewable diff. This
  is what makes FLOW reliable.
- **Parallel agents can't share context.** Each agent starts fresh.
  Two agents independently editing test helpers, config files, or
  CLAUDE.md will conflict.
- **Debugging parallel failures is exponentially harder.** When
  something breaks after sequential tasks, you know which task caused
  it. When something breaks after parallel execution, you have to
  bisect across multiple simultaneous changes.
- **Violates FLOW's design tenets**: "unobtrusive" (adds complex
  orchestration machinery), "safe for local env" (parallel agents
  multiply permission prompts and resource usage).

### Why Option C Adds No Value

Users can already install any Claude Code skill alongside FLOW.
Recommending "install it yourself" isn't an integration — it's a
non-answer. The DAG skill's standalone execution model (its own phases
1-5) conflicts with FLOW's plan mode context, so the experience would
be clunky without structured integration.

## Recommendation: Option A — DAG as Planning Methodology

### What to Extract from mkw-DAG-architect

The full SKILL.md is ~12KB across 5 phases. Most of it handles
standalone execution (activation commands, execution phase, synthesis,
quality scoring) that FLOW already covers through its own phases.
Extract the core planning methodology:

**Take (~2-3KB of enhanced instructions):**

<!-- markdownlint-disable MD013 -->

| Concept | What it does | How it helps FLOW |
|---|---|---|
| **Impact preview** | Quick assessment: does this feature have 4+ meaningful tasks with real dependencies? | Skip DAG analysis for simple features (rename a field, fix a typo). Save tokens and time. |
| **Dependency identification** | For each task, explicitly list what it depends on. Check for hidden cross-cutting dependencies. | Catches ordering mistakes: "Task 5 depends on Task 2 but was listed before it." |
| **Cycle validation** | Verify no circular dependencies exist in the task graph. | Catches impossible orderings: "A depends on B depends on C depends on A." |
| **Topological ordering** | Sort tasks so every task comes after its dependencies. | The linear task list gets provably correct ordering. |
| **Coverage checking** | Verify every aspect of the problem is addressed by at least one task. No orphaned tasks. | Catches missing work: "The API endpoint is built but nothing handles the error case." |
| **Node type categorization** | Categorize tasks as: research, design, implement, test, integrate, validate. | Better task descriptions. Makes it clear which tasks are exploration vs. construction. |

**Leave out:**

| Concept | Why exclude |
|---|---|
| Activation commands (`decompose:`, `decompose preview:`) | FLOW has its own invocation via `/flow:flow-plan` |
| XML DAG format | Too heavyweight; a Markdown dependency table is clearer and fits the plan file |
| Execution phase | FLOW's Code phase handles execution |
| Synthesis phase | The plan file IS the synthesis |
| Step-by-step mode | FLOW Code phase already does one-task-at-a-time |
| Quality scoring per node | Interesting but orthogonal to planning |
| "What vanilla reasoning missed" comparison | Useful standalone, not in FLOW's structured workflow |

<!-- markdownlint-enable MD013 -->

### How the Enhanced Plan Step 2 Would Work

Current Step 2 is: "Explore the codebase, design the approach, and
write the implementation plan."

Enhanced Step 2 becomes:

**2a. Explore the codebase (unchanged)**

Read files, search code, understand patterns. Same as today.

**2b. Complexity assessment (new — from Impact Preview)**

Before writing the plan, assess:

- How many logical tasks does this feature require?
- Do tasks have real dependencies on each other?
- Are there cross-cutting concerns (shared test fixtures, config
  changes, migration ordering)?

If the feature is simple (fewer than 4 tasks, no real dependencies),
skip DAG decomposition and write the linear plan directly. Most bug
fixes, documentation changes, and simple additions fall here.

**2c. DAG decomposition (new — for complex features)**

For features that pass the complexity threshold:

1. **List all tasks** with a type category (research, design,
   implement, test, integrate, validate)
2. **Map dependencies** — for each task, explicitly state what must be
   complete before it can start
3. **Validate** — check for circular dependencies, verify every aspect
   of the problem is covered
4. **Order** — topologically sort tasks so dependencies come first,
   preserving TDD ordering (test task immediately before its
   implementation task)

**2d. Write the plan file (enhanced format)**

The plan file gains an optional "Dependency Graph" section between
Approach and Tasks:

```markdown
## Dependency Graph

| Task | Type | Depends On |
|------|------|------------|
| 1. Write conftest fixtures | design | — |
| 2. Write parser tests | test | 1 |
| 3. Implement parser | implement | 2 |
| 4. Write API tests | test | 1 |
| 5. Implement API endpoint | implement | 3, 4 |
| 6. Integration test | validate | 5 |

Tasks 2 and 4 are independent — but executed sequentially
(FLOW Code phase is linear).
Task 5 depends on both 3 and 4, so it must come after both.
```

The Tasks section remains a linear ordered list — same as today, but
now with provably correct ordering derived from the dependency
analysis.

### Configuration

Add to `.flow.json` skills config:

```json
{
  "skills": {
    "flow-plan": {
      "continue": "manual",
      "dag": "auto"
    }
  }
}
```

- `"auto"` (default) — run complexity assessment, use DAG
  decomposition only for complex features
- `"always"` — always use DAG decomposition
- `"never"` — skip DAG decomposition entirely (current behavior)

The Plan skill reads this from the state file (same pattern as
`continue` mode resolution — prime presets flow through `.flow.json` →
state file → skill reads).

## Implementation Plan

### Files to Modify

| File | Change |
|------|--------|
| `skills/flow-plan/SKILL.md` | Add DAG decomposition instructions to Step 2 (split into 2a-2d) |
| `skills/flow-prime/SKILL.md` | Add `dag` config to prime presets |
| `tests/test_skill_contracts.py` | Add contract test for DAG decomposition presence in Plan skill |
| `docs/skills/flow-plan.md` | Document DAG decomposition feature |
| `docs/phases/phase-2-plan.md` | Update phase docs |
| `README.md` | Mention DAG-enhanced planning in features |
| `docs/index.html` | Update if needed for feature keywords |

### Files Unchanged

- `skills/flow-code/SKILL.md` — no changes to Code phase
- `flow-phases.json` — no phase structure changes
- `lib/*.py` — no new scripts needed (DAG decomposition is
  instructions, not code)
- State file schema — no new fields (dag config follows existing
  skills config pattern)

### Tasks (TDD Order)

1. **Test**: Add contract test in `test_skill_contracts.py` for
   DAG-related sections in Plan skill
2. **Implement**: Enhance `skills/flow-plan/SKILL.md` Step 2 with DAG
   decomposition instructions
3. **Implement**: Add `dag` config to prime presets in
   `skills/flow-prime/SKILL.md`
4. **Test**: Verify `bin/flow ci` passes (existing tests + new
   contract test)
5. **Docs**: Update `docs/skills/flow-plan.md`,
   `docs/phases/phase-2-plan.md`
6. **Docs**: Update `README.md` and `docs/index.html` for doc sync
   tests

### Verification

- `bin/flow ci` green (includes contract tests, doc sync, permissions)
- Manual test: run `/flow:flow-plan` on a complex feature and verify
  the plan includes dependency graph
- Manual test: run `/flow:flow-plan` on a simple feature and verify
  DAG decomposition is skipped
