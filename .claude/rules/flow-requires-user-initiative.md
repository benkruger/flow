# Flow Requires User Initiative

FLOW skills divide into two authorization tiers based on the action
they perform.

## User-Only Skills (model must never invoke)

Four skills must be invoked directly by the user — the model never
proposes them and never invokes them, even after hypothetical
approval. See `.claude/rules/user-only-skills.md` for the full set
and the three-layer mechanical enforcement chain
(`validate-skill`, `validate-ask-user` carve-out, transcript root
lockdown).

The user-only set:

- `/flow:flow-abort`
- `/flow:flow-reset`
- `/flow:flow-release`
- `/flow:flow-prime`

## Ask-First Skills (model may invoke after user agreement)

The remaining lifecycle-initiating skills follow an ask-first
pattern: never invoke unless the user explicitly asks. If you
believe a change should go through the FLOW lifecycle, say so —
but let the user decide. Never auto-start a flow because you think
the change is big enough to warrant one.

The ask-first family includes:

- `/flow:flow-start`
- `/flow:flow-create-issue`

Distinguishing test: would the model proposing the action embarrass
or surprise the user if their answer were "no"? If the answer is
"yes, this would be lost work or a public artifact" → user-only.
If the answer is "no, it would be a wasted prompt" → ask-first.
