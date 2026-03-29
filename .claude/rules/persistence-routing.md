# Persistence Routing

Decision tree for where to store information:

1. **Is it specific to this user?** (preferences, role, communication
   style, corrections about working together) → **Memory**
2. **Is it a behavioral constraint?** (do X, never do Y, when X
   happens do Y — an imperative guardrail) → **Rule**
3. **Is it project knowledge?** (architecture, key files, how things
   connect, conventions) → **CLAUDE.md**

## Tests

- Can you phrase it as an imperative? → Rule, not CLAUDE.md
- Should every engineer follow this, or just this user? → Rule if
  everyone, Memory if just this user
- Can you derive it by reading code, CLAUDE.md, or rules? → Don't
  save it anywhere

## Never Store in Memory

- Architecture, code facts, or file paths — read the code
- Duplicates of existing rules or CLAUDE.md content
- Git history or debugging solutions
- Ephemeral task state
