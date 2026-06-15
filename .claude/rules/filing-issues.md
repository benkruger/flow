# Filing Issues

"Let's brainstorm/think about/what if" = discussion, NOT filing — don't invoke any
filing skill until the user says "file/create an issue". Default to inclusion when
scoping (see `include-bias-in-issues.md`). After a `decompose` run, file via
`/flow:flow-explore` or `/flow:flow-plan`, never bare `bin/flow issue` (that
discards the pre-planning).

Mechanics: write the body to `<worktree>/.flow-issue-body` (absolute path) with the
Write tool, then `bin/flow issue --title "..." --body-file <abs-path>`. Never pass
body text as a CLI argument; never delete the body file on create (the script
does); always create via `bin/flow issue`, never `gh issue create`. Editing an
existing issue: write the body, `gh issue edit <N> --body-file <abs>`, then dispose
of the file via `bin/flow delete-body-file --path <abs>`. Never write temp files to
`/tmp/`.

Content: issues are bug reports, not design docs — what is broken, observable
behavior + evidence, repro steps, files to investigate (not to change), no
solutioning. (Exception: decomposed issues from `flow-plan` carry an Implementation
Plan.) Verify the root cause by reading code before filing.

A hook/gate block is presumptively intentional — read the hook doc, its rule, and
its test before filing it as broken; name a specific authorized-safe input it
wrongly fires on. Friction (a scanner firing, an opt-out you had to type) is not a
process gap. Value test: a gap caught by another phase gate AND fixed in this PR →
record, don't file.

Repo routing: default to the current repo (omit `--repo`); FLOW plugin bugs →
`--repo benkruger/flow`. Dependencies: `bin/flow link-blocked-by`, not a label.
