# Hook vs Instruction

## When to Use a PreToolUse Hook

Use a hook — not a skill instruction — when:

- Claude ignoring the instruction causes a permission prompt
  or blocks the flow
- The behavior must be enforced across all phases, skills,
  and sub-agents universally
- You find yourself adding the same instruction to multiple
  skills independently
- The consequence of non-compliance is user-visible (blocked
  prompt, wrong file edited, permission denied)

## When Skill Instructions Suffice

Use skill instructions when:

- The behavior is specific to one phase or step
- Non-compliance is annoying but not flow-blocking
- The instruction is contextual (depends on plan content,
  user preferences, or phase-specific state)

## The Principle

Skill instructions are advisory — Claude can ignore them.
Hooks are enforcement — they run as code before the tool
executes. If "Claude might not follow this" has a
consequence that blocks the user, it must be a hook.
