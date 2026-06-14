# Agent Command Paths

A command embedded in a FLOW sub-agent's prompt for the sub-agent to
run must not carry the plugin-root prefix (an unexpanded
`bin/flow`-locating env-var token): the child session does not expand
it, so the literal token fails. The parent resolves the absolute plugin
`bin/flow` path via `bin/flow plugin-bin-flow` and substitutes it into
the agent prompt.

- Resolve it in the parent SKILL bash fence (the env var expands
  there); capture the trimmed stdout — an absolute `…/bin/flow` path.
- Substitute that path into the agent prompt; the sub-agent runs it
  verbatim.
- If `plugin-bin-flow` exits non-zero (the plugin root is unset, empty,
  or non-absolute), surface the error and halt — never embed the
  non-path error string in the prompt and never fall back to the
  unexpanded token.

Parent SKILL bash fences (run by Claude Code, not a sub-agent) keep the
plugin-root prefix — expansion works there.
