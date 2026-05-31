# No Placeholder Anchors

Never write a placeholder file to anchor a later tool-output
redirect. The pattern is forbidden regardless of destination
(`/tmp/`, `.flow-states/`, anywhere else) and regardless of how the
placeholder is created (Write tool, `touch`, `echo > path`).

## The Forbidden Pattern

A placeholder anchor is a file the model creates with empty or
trivial content, intending to redirect a later command's output
into the same path. Canonical shapes:

- Write tool creates `/tmp/output.txt` with empty content, then a
  later bash command tries `<some command> > /tmp/output.txt`.
- Write tool creates `.flow-states/<branch>/scratch.json` as a
  stub, then the model intends to `gh issue view <N> > <stub>`.
- The model writes a small comment line into a file expecting to
  append real output through a redirect later.

In every case the intent is "anchor the destination now, route
content into it later via a shell construct."

## Why It Is Forbidden

Two independent reasons make the pattern wrong regardless of
destination.

**Shell redirection is blocked by `validate-pretool`.** The
PreToolUse hook rejects `>`, `>>`, `<`, and `tee` outside quoted
arguments. The intended follow-up redirect never runs, so the
placeholder anchors nothing. The model is then tempted to work
around the block (creating wrapper scripts, expanding allow
lists), which compounds the problem.

**Machine-global paths race between concurrent flows.** Per
`.claude/rules/concurrency-model.md` "Before Writing Any Code,"
fixed `/tmp/<name>` paths collide when two flows on the same
machine run simultaneously — the second flow's placeholder
overwrites the first, and either flow can read either flow's
content depending on timing. Even `.flow-states/<branch>/` is
not the right destination for a placeholder-then-redirect
pattern, because the redirect itself is blocked at the hook
layer; the placeholder accomplishes nothing.

**The harness already persists large tool output.** The Bash
tool writes its full stdout and stderr to a log file that the
Read tool can scan after the fact. For `bin/flow ci` and its
single-phase variants, the path is
`<project_root>/.flow-states/<branch>-ci-last.log`. For other
commands, the output is available inline or by running narrower
passes. The destination the placeholder was meant to anchor does
not need to exist on disk — the existing artifact is the
documented surface.

## How to Apply

When the urge to "create a file now, redirect into it later"
arises:

1. **Identify the tool whose output you need.** If it's a
   `bin/flow` subcommand, the runner already persists its output;
   use the Read tool on the documented log path.
2. **For ad-hoc commands, narrow the scope or use Grep.** A grep
   over a specific directory returns structured output that
   bypasses the Bash display buffer; a directory-by-directory
   pass returns results inline.
3. **For real persistent artifacts the user expects to find on
   disk**, write the content directly via the Write tool —
   without an empty placeholder intermediate step. The Write
   tool creates the file in one atomic action; there is no
   "anchor then fill" middle state.
4. **For FLOW-managed artifacts** (`plan.md`,
   `commit-msg.txt`, `.flow-issue-body`, `orchestrate-queue.json`),
   route through `bin/flow write-rule` per
   `.claude/rules/file-tool-preflights.md`. That subcommand reads
   the content file and writes the canonical destination from
   inside Rust — no placeholder, no redirect.

If none of those paths fits the operation, the operation is
asking the wrong question. The fix is in the design upstream —
either the command needs to produce its output differently, or
the consumer should read from the artifact the command already
produces, or the work doesn't belong in this skill at all.

## Cross-References

- `.claude/rules/permissions.md` "Symmetric R+W /tmp/ Extension
  Policy" — the permission policy that the placeholder anchor
  would route around. The closed extension set is what makes
  the redirect surface narrow enough for a placeholder to look
  like a workaround.
- `.claude/rules/file-tool-preflights.md` — the sanctioned
  surface for managed-artifact writes (`bin/flow write-rule`).
- `.claude/rules/concurrency-model.md` — the broader concurrency
  rationale that forbids fixed-path artifacts under `/tmp/`.
- `.claude/rules/autonomous-phase-discipline.md` "System-Initiated
  Prompts" — the principle that classifies placeholder-then-
  redirect as a model-reaches-for-unsanctioned-operation case
  whose fix is "remove the unsanctioned operation at source."
