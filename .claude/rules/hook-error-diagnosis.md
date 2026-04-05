# Hook Error Diagnosis

When a PreToolUse hook fires and blocks a tool call, the error header
names the specific tool matcher (e.g. `PreToolUse:Edit`,
`PreToolUse:Bash`). Read that matcher literally and map it to the
corresponding hook entry in `hooks/hooks.json` before investigating
anything.

## Matcher to hook mapping

- `PreToolUse:Edit` and `PreToolUse:Write` → `validate-worktree-paths`
  and `validate-claude-paths`
- `PreToolUse:Bash` and `PreToolUse:Agent` → `validate-pretool`
- `PreToolUse:Read`, `PreToolUse:Glob`, `PreToolUse:Grep` →
  `validate-worktree-paths`
- `PreToolUse:AskUserQuestion` → `validate-ask-user`

## Procedure

1. Read the error header — it always names the tool and matcher.
2. Look up that matcher in `hooks/hooks.json` to identify the script.
3. Investigate only that script. Do not read unrelated hook code.
4. Do not guess which hook fired from the error message alone.

Mismatching the matcher to the wrong hook script wastes investigation
time and produces incorrect fixes. Every hook script targets a specific
tool family; the matcher in the error header is the authoritative
pointer.
