# Anti-Patterns

## Inline Output

When a phase produces output the user needs to review (plan file,
DAG analysis, review findings), render the full content inline in
the response text. Never tell the user to "look at" a file path
or "take a look" at a location — the user cannot see file contents
unless they are rendered in the response. The Read tool output
appears in tool results, but users expect the content presented as
formatted text in the response itself.

## Fix Before Remove

When a feature is broken, the first response must be to fix it —
not to remove it. Proposing removal as the initial approach
discards the user's intent. Only propose removal after demonstrating
that the feature cannot be fixed or after the user explicitly asks
for it.
