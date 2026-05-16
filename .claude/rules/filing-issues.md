# Filing Issues

## Brainstorming Is Not Filing

When the user says "lets brainstorm", "lets think about", or "what
if we" — they want a discussion, not a workflow. Do not invoke
`flow:flow-explore`, `flow:flow-plan`, `flow:flow-decompose-project`,
`decompose:decompose`, or any filing skill. Discuss the idea
interactively. Only invoke filing skills when the user explicitly
says "file an issue" or "create an issue."

## Default to Inclusion

When drafting an issue body or scoping a Plan-phase task list,
default to including adjacent concerns rather than excluding
them. The question is not "should this be in scope?" but "is
there a *concrete* reason this must NOT be in scope?" See
`.claude/rules/include-bias-in-issues.md` for the principle, the
bad-reasoning patterns to avoid (prior-PR boundaries, code
ownership, separate code surface, would-expand-scope), and the
narrow valid exclusions. The lifecycle cost of splitting a
concern out of an issue is multiples larger than including it
in the current flow's exploration budget.

## After Decompose Output

When filing issues that originated from a `/decompose:decompose`
analysis in the current conversation, always use
`/flow:flow-explore` (vanilla problem statement), `/flow:flow-plan`
(decomposed implementation plan), or `/flow:flow-decompose-project`
(epic decomposition) — never bare `bin/flow issue`. The decompose
output IS the pre-planning. Filing without it discards the
exploration, risks, approach, and task breakdown that the decompose
produced.

The signal: if the conversation contains a DAG synthesis with
codebase exploration, file references, and an approach — the
issues are pre-planned by definition.

## The Pattern

`bin/flow issue --body-file <path>` resolves a relative `<path>`
against the caller's current working directory (the Bash tool's
cwd at invocation time); absolute paths are used as given. Inside
an active FLOW worktree, flow-start anchors Bash cwd to the
worktree root, so both the Write tool and `bin/flow issue` see
the same directory and a relative `.flow-issue-body-<id>`
resolves consistently between them. Outside a worktree, Bash cwd
is the project root, so a relative path likewise resolves
consistently. The `validate-worktree-paths` hook still blocks
Write-tool calls targeting the main repo when invoked from
inside a worktree, so paths written via the Write tool must land
inside the worktree.

Recommended absolute-path shape (unambiguous regardless of where
Bash cwd ends up):

1. Write the issue body to `<worktree>/.flow-issue-body` (or
   `<worktree>/.flow-issue-body-1`, etc., for parallel filing)
   using the Write tool — the absolute worktree path satisfies
   the `validate-worktree-paths` hook
2. Call `bin/flow issue --title "..." --body-file
   <worktree>/.flow-issue-body` using the absolute worktree path
3. The script reads the file, deletes it on a best-effort basis
   (per `read_body_file`'s doc comment — cleanup errors are
   swallowed), then creates the issue

Relative paths are accepted when the Bash cwd matches the Write
target's parent directory. `bin/flow issue` rejects `..` traversal
segments in relative paths, rejects empty `--body-file` arguments,
and rejects non-regular-file targets (symlinks, directories,
device nodes) via `fs::symlink_metadata` — see
`.claude/rules/external-input-path-construction.md`.

## Editing Existing Issues

Use the same `.flow-issue-body` temp file pattern with the same
absolute-worktree-path discipline described above:

1. Write the updated body to `<worktree>/.flow-issue-body` using the Write tool
2. Call `gh issue edit <number> --repo <owner/repo> --body-file
   <worktree>/.flow-issue-body`
3. Delete `<worktree>/.flow-issue-body` yourself — `gh issue edit`
   does not auto-delete

Never write temp files to `/tmp/` — the project's `defaultMode:
"plan"` has no allow-list pattern for `/tmp/` paths, triggering
permission prompts.

## Rules

- Never pass body text as a command line argument — special
  characters trigger the Bash hook validator
- Never delete `.flow-issue-body` yourself when creating — the
  script handles cleanup after reading
- Always use `bin/flow issue` for creating — never call
  `gh issue create` directly

## Content Standards

Issues are bug reports, not design documents. Capture
the problem with zero solutioning. Research, diagnosis,
and design happen in the Plan phase after proper codebase
exploration.

**Exception: Decomposed issues.** Issues filed by
`flow-plan` or `flow-decompose-project` include an
Implementation Plan section (Context, Exploration, Risks,
Approach, Dependency Graph, Tasks). This is the only context
where solution design belongs in an issue body — these issues
are pre-planned for fast-tracking through the Plan phase.

- **Write for a cold start.** A future session has no
  memory of this conversation. The issue is its only
  context for the problem.
- **Describe what is broken and why it matters.** Include
  observable behavior, evidence (state file values, error
  messages, logs), and user impact.
- **Include reproduction steps.** Steps or conditions that
  trigger the problem.
- **Name files to investigate, not files to change.** Point
  to where the behavior originates so the Plan phase knows
  where to start reading.
- **File independent issues in parallel.** Use different
  temp file names (e.g., `.flow-issue-body-1`,
  `.flow-issue-body-2`) and launch all Write + `bin/flow
  issue` calls concurrently.

## Verify Before Filing

When filing a bug discovered during a FLOW phase (Review
tech debt, Learn process gaps), read the relevant source code
and verify the root cause before filing. A hypothesis about
what might be happening is not evidence. The issue body must
contain the verified mechanism — file path, line number, and
what the code actually does — not a guess about what it might
do. A cold-start session should be able to act on the issue
without re-doing the investigation.

## Mechanical Blocks Are Presumptively Intentional

When the bug under investigation is "a hook, gate, or guard
blocked an action," the default assumption is that the block is
intentional. Hooks and gates in this codebase are the
enforcement layer for rules the project has chosen — they are
designed user-handoff surfaces, not defects. A working
enforcement mechanism is not a bug.

Before filing an issue that describes a block as broken, do
four things:

1. **Read the hook's module doc.** The Rust file that emits the
   block (typically `src/hooks/<name>.rs`) names what failure
   mode the block prevents.
2. **Read the rule that authorizes the block.** The rule file
   in `.claude/rules/` cited by the module doc describes the
   design intent in prose.
3. **Read the test that locks the block in.** The test file at
   `tests/hooks/<name>.rs` shows the canonical block cases and
   their authorized inputs.
4. **Name the case that falls outside that intent.** The
   filable issue must point to a specific input the block fires
   on that the rule and tests do NOT authorize as a target.

The following framings are NOT valid grounds for filing:

- "The block was inconvenient."
- "The model couldn't proceed autonomously."
- "I wanted it to ask the user instead of blocking."
- "The recovery path requires user intervention."
- "The flow stalled until the user typed a continue token."

Each of these describes the block doing its job. A user-handoff
IS the designed recovery path for the situations these blocks
catch — the human reading the conversation is the authorized
recovery channel, not an `AskUserQuestion` carve-out the model
can take on its own.

A genuine block-related defect looks like one of:

- The block fires on an input the rule and tests explicitly
  authorize as a safe case (false positive against documented
  scope).
- The block message points the user at a recovery action that
  no longer exists or never worked.
- Two hooks emit contradictory directives that produce a
  genuine deadlock — one hook demands an action, another hook
  blocks the same action — AND no existing carve-out resolves
  the contradiction.

When in doubt, do not file. A working block surfaced as an
issue costs triage time on every future session that opens the
issue list; a real block-related defect will be filed again the
next time it fires with the right framing.

## Value Test Before Filing

Before filing any issue derived from a FLOW phase finding,
apply the same value test that
`.claude/rules/review-scope.md` applies to Review
findings — adapted for the issue-filing decision:

> **Was the gap caught by another phase gate AND remediated in
> this PR (code fix, rule clarification, or new rule)?**
>
> - If yes → the system already closed the gap. Record it in
>   the commit message and the Learn report. Do not file an
>   issue.
> - If no → the gap is open. File it.

The trap to avoid: framing "Plan phase didn't catch X but Code
Review did" as a process gap. Review IS part of the
process. The cognitive-isolation design (four sub-agents)
exists precisely to catch what the Plan author missed. A
Review-caught-and-fixed violation is the system working,
not a gap.

A real Tenant 1 process gap looks like one of:

- A class of bug where no phase gate would have caught it —
  the bug shipped, was discovered post-merge, and nothing in
  the existing process would have prevented it.
- A workflow step that broke (skill error, hook misfire, state
  corruption) and the error path was undefined or destructive.
- An async/dangling operation: a background agent that never
  reported back, a state mutation without a paired commit, a
  notification that never fired.

### Friction Is Not a Process Gap

A scanner that fires correctly, an opt-out the author had to
type, a repair round that resolved the violation — these are
the gate doing its job. They are NOT process gaps, even when
the friction feels excessive in a single flow. Specifically,
none of the following count as Tenant 1 findings:

- "The scanner over-fired and I had to add N opt-out comments."
- "A contract test rejected my change twice before it cleared."
- "The rule required me to enumerate X items in a table."
- "I had to write a manual workaround the rule documents."

These are the cost of the rule the project chose to enforce.
The rule already has an opt-out grammar OR a documented
workaround OR a published cost; using it is not a gap.

A friction report becomes filable only when ALL three hold:

1. **Recurrence across flows.** The same friction has been
   observed in three or more separate flows by Learn-phase
   findings, not anticipated as one-off in the current flow.
2. **Cost is disproportionate.** The opt-out count, repair
   rounds, or workaround steps exceed what the rule's design
   intended (read the rule file's "How to Apply" or "Trigger"
   section — if the friction is what the rule names as the
   cost, it is intended).
3. **A concrete cheaper enforcement exists.** The filer can
   name a specific scanner refinement, opt-out grammar
   extension, or rule-vocabulary change that would reduce
   friction without losing the gate's protective intent.

If any of (1)–(3) is missing, do not file. Single-flow
friction is not a signal — it is one data point, and the
project's curated-closed scanner philosophy explicitly prefers
some friction over false-positive sweeps from premature
scanner expansion.

A real Tenant 2 enforcement escalation looks like:

- A rule that was clear, applicable, and ignored AND the same
  violation has been observed across multiple flows — pattern,
  not one-off — AND instruction-level enforcement has
  demonstrably failed to fix it.

When in doubt, do not file. The cost of an un-filed real issue
is one more flow surfacing it; the cost of a filed bogus issue
is triage time on every future session that opens the issue
list.

## Repo Routing

Most issue-filing paths target the current project (omit `--repo`):
Tech Debt, Documentation Drift, and decomposed work items all
describe problems in the user's code.

FLOW process bugs — problems with the plugin itself — must target
`benkruger/flow`. Pass `--repo benkruger/flow` when filing against
the plugin repo. The Phase 4 `flow-learn` skill is the only skill
that routes process gaps cross-repo automatically:

- `flow-learn` (Phase 4) — files process gap issues with `--repo`

`flow-explore`, `flow-plan`, and `flow-decompose-project` always
file to the **current** repo (no `--repo` flag) — they do not
prompt for a target. To file a FLOW process bug manually from a
target project, invoke `bin/flow issue --repo benkruger/flow`
directly rather than going through `flow-explore`, `flow-plan`,
or `flow-decompose-project`.

When in doubt, ask the user. Filing against the wrong repo is
worse than one extra question.

## Dependencies

When filing an issue that depends on another issue, set the native
blocked-by relationship with `bin/flow link-blocked-by`:

```bash
bin/flow link-blocked-by --repo <owner/repo> \
  --blocked-number <blocked> --blocking-number <blocking>
```

`flow-issues` detects blocked status from GitHub's native
`blockedBy` relationship — no "Blocked" label is required.

## Never Include

These rules apply to standard issues. Decomposed issues filed
by `flow-plan` or `flow-decompose-project` are exempt — they
include an Implementation Plan section by design.

- Root cause analysis — a guess is not analysis
- Proposed solutions or "open questions" about tradeoffs
- Prescribed code changes or architectural suggestions
- Diagnosis of why the bug happens — only what happens
</content>
</invoke>